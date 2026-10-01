//! Drop a file on Glowby.
//!
//! While you drag a file, Windows sends no normal mouse messages (the drag
//! "captures" the mouse), so the hover strip can't notice you. Instead the strip
//! registers as an OLE *drop target*: Windows calls `DragEnter` when a drag
//! reaches it, Glowby slides out, and the drop itself lands on the pet window
//! (Tauri reports it as a DragDrop window event).

use crate::state::{self, AppState, lock};
use crate::{chat, pet_window};
use tauri::{AppHandle, DragDropEvent, Manager};
use windows::Win32::Foundation::{HWND, POINTL};
use windows::Win32::System::Com::IDataObject;
use windows::Win32::System::Ole::{DROPEFFECT, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl, OleInitialize, RegisterDragDrop};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::core::{Ref, implement};

/// COM object Windows calls while something is dragged over the hover strip.
#[implement(IDropTarget)]
struct EdgeDropTarget {
    app: AppHandle,
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for EdgeDropTarget_Impl {
    fn DragEnter(&self, _data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe { *effect = DROPEFFECT_NONE }; // the strip itself accepts nothing
        if lock(&self.app.state::<AppState>().settings).drop_files {
            pet_window::show(&self.app);
        }
        Ok(())
    }

    fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe { *effect = DROPEFFECT_NONE };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        Ok(())
    }

    fn Drop(&self, _data: Ref<'_, IDataObject>, _keys: MODIFIERKEYS_FLAGS, _pt: &POINTL, effect: *mut DROPEFFECT) -> windows::core::Result<()> {
        unsafe { *effect = DROPEFFECT_NONE };
        Ok(())
    }
}

/// Must run on the main thread, after the hover strip exists.
pub fn register_edge(app: &AppHandle) {
    let zone = crate::hotzone::hwnd();
    if zone.is_null() {
        return;
    }
    unsafe {
        let _ = OleInitialize(None); // already done by Tauri; harmless if repeated
        let target: IDropTarget = EdgeDropTarget { app: app.clone() }.into();
        if let Err(e) = RegisterDragDrop(HWND(zone as _), &target) {
            crate::applog::line(format!("drag-to-summon unavailable: {e}"));
        }
    }
}

/// Drag events on the pet window itself.
pub fn on_pet_drag(app: &AppHandle, event: &DragDropEvent) {
    let state = app.state::<AppState>();
    if !lock(&state.settings).drop_files {
        return;
    }
    match event {
        DragDropEvent::Enter { paths, .. } if !paths.is_empty() => {
            lock(&state.ui).drop_hover = true;
            state::publish(app);
        }
        DragDropEvent::Drop { paths, .. } => {
            lock(&state.ui).drop_hover = false;
            if !paths.is_empty() {
                chat::attach(app, paths.clone());
                lock(&state.ui).chat_open = true;
                pet_window::focus_for_typing(app);
            }
            state::publish(app);
        }
        DragDropEvent::Leave => {
            lock(&state.ui).drop_hover = false;
            state::publish(app);
        }
        _ => {}
    }
}
