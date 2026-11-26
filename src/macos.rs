//! Arquivos abertos pelo Finder (duplo clique, "Abrir com", soltar no ícone
//! do Dock). No macOS o caminho não chega nos argumentos do programa: vem num
//! Apple Event "abrir documentos", que o winit não trata. Aqui ele é recebido
//! e os arquivos vão para uma fila que o app esvazia a cada quadro.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSString};

// Códigos de quatro letras dos Apple Events (AERegistry.h, AEDataModel.h).
const CORE_EVENT_CLASS: u32 = u32::from_be_bytes(*b"aevt");
const OPEN_DOCUMENTS: u32 = u32::from_be_bytes(*b"odoc");
const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static CTX: OnceLock<egui::Context> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CsvViwerOpenHandler"]
    struct OpenHandler;

    impl OpenHandler {
        #[unsafe(method(willFinishLaunching:))]
        fn will_finish_launching(&self, _note: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            unsafe {
                let _: () = msg_send![
                    &*manager,
                    setEventHandler: self,
                    andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                    forEventClass: CORE_EVENT_CLASS,
                    andEventID: OPEN_DOCUMENTS
                ];
            }
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
            let list: Option<Retained<NSAppleEventDescriptor>> =
                unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
            let Some(list) = list else { return };
            // Normalmente é uma lista (índices a partir de 1), mas pode vir um item só.
            let n = list.numberOfItems();
            let items: Vec<_> = if n > 0 {
                (1..=n).filter_map(|i| list.descriptorAtIndex(i)).collect()
            } else {
                vec![list]
            };
            let paths: Vec<PathBuf> = items
                .iter()
                .filter_map(|d| d.fileURLValue()?.to_file_path())
                .collect();
            if paths.is_empty() {
                return;
            }
            OPENED.lock().unwrap().extend(paths);
            if let Some(ctx) = CTX.get() {
                ctx.request_repaint();
            }
        }
    }
);

impl OpenHandler {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

/// Começa a receber os arquivos; chamar no `main`, antes de abrir a janela.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let handler = OpenHandler::new(mtm);
    // O AppKit instala os próprios tratadores ao iniciar e sobrescreveria um
    // instalado antes; os documentos da abertura são entregues logo depois de
    // NSApplicationWillFinishLaunchingNotification, então é nela que se instala.
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &handler,
            sel!(willFinishLaunching:),
            Some(&name),
            None,
        );
    }
    // Nem o AppKit nem a central de notificações retêm o objeto.
    std::mem::forget(handler);
}

/// Contexto usado para acordar a janela quando chegam arquivos com o app aberto.
pub fn set_context(ctx: &egui::Context) {
    let _ = CTX.set(ctx.clone());
}

/// Arquivos recebidos desde a última chamada.
pub fn take_opened_files() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap())
}
