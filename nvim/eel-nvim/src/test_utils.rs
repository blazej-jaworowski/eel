use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use tracing::debug;

use crate::{editor::NvimEditor, lua::lua_get_global_path};

pub fn run_nvim_async_test<F, T>(test: F)
where
    F: FnOnce(NvimEditor) -> T,
    T: Future<Output = ()> + Send + 'static,
{
    eel::tracing::init_tracing([eel::tracing::file_log_layer("/tmp/eel")]);

    eel::async_runtime::init_runtime().expect("Failed to initialize async runtime");
    let editor = NvimEditor::new_on_current().expect("Failed to initialize NvimEditor");

    let test = test(editor);
    let out = Arc::new(AtomicBool::new(false));

    let test_handle = {
        let out = out.clone();
        eel::async_runtime::spawn(async move {
            debug!("Running test future");

            test.await;

            debug!("Test successfully finished");

            out.store(true, Ordering::Release);
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

    if !wait_result {
        test_handle.abort();
    }

    assert!(wait_result, "Test timed out");
    assert!(out.load(Ordering::Acquire), "Test didn't succeed")
}
