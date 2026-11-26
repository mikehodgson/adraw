//! Finder sends "Open With" files as an Apple Event instead of process
//! arguments, and winit does not forward it, so ADraw handles it directly.

use eframe::egui;
use objc2::{define_class, msg_send, rc::Retained, runtime::NSObject, sel, AllocAnyThread};
use objc2_foundation::{
    NSAppleEventDescriptor, NSAppleEventManager, NSNotification, NSNotificationCenter, NSString,
};
use std::{
    path::PathBuf,
    sync::{Mutex, OnceLock, PoisonError},
};

const fn four_char_code(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const CORE_EVENT_CLASS: u32 = four_char_code(b"aevt");
const OPEN_DOCUMENTS: u32 = four_char_code(b"odoc");
const DIRECT_OBJECT: u32 = four_char_code(b"----");

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

define_class!(
    // SAFETY: NSObject has no subclassing requirements and Handler does not
    // implement Drop.
    #[unsafe(super(NSObject))]
    #[name = "ADrawOpenDocumentsHandler"]
    struct Handler;

    impl Handler {
        // AppKit installs its default handlers while launching, so ours must
        // replace them in response to this notification.
        #[unsafe(method(applicationWillFinishLaunching:))]
        fn will_finish_launching(&self, _notification: &NSNotification) {
            let manager = NSAppleEventManager::sharedAppleEventManager();
            unsafe {
                let _: () = msg_send![
                    &manager,
                    setEventHandler: self,
                    andSelector: sel!(handleOpenDocuments:withReplyEvent:),
                    forEventClass: CORE_EVENT_CLASS,
                    andEventID: OPEN_DOCUMENTS
                ];
            }
        }

        #[unsafe(method(handleOpenDocuments:withReplyEvent:))]
        fn handle_open_documents(
            &self,
            event: Option<&NSAppleEventDescriptor>,
            _reply: Option<&NSAppleEventDescriptor>,
        ) {
            let Some(event) = event else { return };
            let files: Option<Retained<NSAppleEventDescriptor>> =
                unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
            let Some(files) = files else { return };
            let count = files.numberOfItems();
            let items: Vec<_> = if count == 0 {
                vec![files]
            } else {
                (1..=count)
                    .filter_map(|index| files.descriptorAtIndex(index))
                    .collect()
            };
            let paths: Vec<_> = items
                .iter()
                .filter_map(|item| item.fileURLValue()?.path())
                .map(|path| PathBuf::from(path.to_string()))
                .collect();
            if paths.is_empty() {
                return;
            }
            OPENED
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(paths);
            if let Some(ctx) = CONTEXT.get() {
                ctx.request_repaint();
            }
        }
    }
);

/// Registers for Finder open requests; call before the event loop starts.
pub fn install() {
    let handler = Handler::alloc().set_ivars(());
    let handler: Retained<Handler> = unsafe { msg_send![super(handler), init] };
    let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
    unsafe {
        NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
            &handler,
            sel!(applicationWillFinishLaunching:),
            Some(&name),
            None,
        );
    }
    // Neither the notification center nor the Apple Event manager retains it.
    std::mem::forget(handler);
}

pub fn set_context(ctx: egui::Context) {
    let _ = CONTEXT.set(ctx);
}

/// Files Finder asked ADraw to open since the last call.
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(PoisonError::into_inner))
}
