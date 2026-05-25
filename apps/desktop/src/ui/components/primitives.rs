use std::fmt::Display;

use floem::prelude::*;

#[allow(clippy::disallowed_methods)]
pub fn ui_button(child: impl IntoView + 'static) -> Button {
    button(child)
}

#[allow(clippy::disallowed_methods)]
pub fn ui_label<S: Display + 'static>(content: impl Fn() -> S + 'static) -> Label {
    label(content)
}

#[allow(clippy::disallowed_methods)]
pub fn ui_static_label(content: impl Into<String>) -> Label {
    static_label(content)
}

#[allow(clippy::disallowed_methods)]
pub fn ui_text_input(buffer: RwSignal<String>) -> TextInput {
    text_input(buffer)
}

#[allow(clippy::disallowed_methods)]
pub fn ui_text_editor(text: String) -> TextEditor {
    text_editor(text)
}
