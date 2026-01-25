use std::sync::{Arc, mpsc};

use eel::{
    Editor,
    test_utils::{EditorFactory, EditorTest},
};
use tracing::debug;

use crate::{editor::NvimEditor, lua::lua_get_global_path};

pub fn run_nvim_test<E, EF, T, R>(test: T, editor_factory: EF) -> R
where
    E: Editor,
    EF: EditorFactory<Editor = E>,
    T: EditorTest<E, R>,
    R: Send + 'static,
{
    eel::tracing::init_tracing([eel::tracing::file_log_layer("/tmp/eel")]);

    let editor = editor_factory.create_editor();

    let (send, recv) = mpsc::channel();

    let test_handle = {
        std::thread::spawn(move || {
            debug!("Running test");

            let result = test.run(editor);

            debug!("Test successfully finished");

            send.send(result).expect("Test result send error");
        })
    };

    let test_handle = Arc::new(test_handle);

    let wait_func: nvim_oxi::mlua::Function =
        lua_get_global_path("vim.wait").expect("Failed to get vim.wait");

    let cond_func = {
        let test_handle = test_handle.clone();
        nvim_oxi::mlua::lua()
            .create_function(move |_, ()| Ok(test_handle.is_finished()))
            .expect("Failed to create test lua function")
    };

    let wait_result: bool = wait_func
        .call((1000, cond_func))
        .expect("Failed to call vim.wait");

    assert!(wait_result, "Test timed out");

    recv.try_recv().expect("Failed to get test result")
}

pub fn nvim_editor_factory() -> NvimEditor {
    NvimEditor::new_on_current().expect("Failed to initialize editor")
}
