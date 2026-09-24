// DOM UI for TPT CAD Format. Phase 0 placeholder: a mounted shell that proves
// the hub contract end-to-end; the real upload/validator UI replaces this in
// Phase 2.
use std::rc::Rc;
use tpt_appfront_core::UITree;
use wasm_bindgen::prelude::*;

// The placeholder shell is static — nothing dispatches messages.
#[derive(Debug, Clone)]
enum Msg {}

pub fn mount_app(container: &web_sys::Element) -> Result<(), JsValue> {
    let ui: UITree<Msg> = UITree::container(|c| {
        c.heading(1, "TPT CAD Format");
        c.text("Validator shell loaded.");
    });
    let dispatch: Rc<dyn Fn(Msg)> = Rc::new(|_| {});
    let root = tpt_appfront_dom::mount(container, &ui, dispatch)?;
    std::mem::forget(root);
    Ok(())
}
