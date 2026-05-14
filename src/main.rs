use tower_lsp::{LspService, Server};

mod document;
mod php;
mod server;

const LSP_THREAD_STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() {
    std::thread::Builder::new()
        .name("rephactor-lsp".to_string())
        .stack_size(LSP_THREAD_STACK_SIZE)
        .spawn(run_lsp_server)
        .expect("spawn rephactor lsp thread")
        .join()
        .expect("rephactor lsp thread panicked");
}

fn run_lsp_server() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build tokio runtime");

    runtime.block_on(async {
        let stdin = tokio::io::stdin();
        let stdout = tokio::io::stdout();

        let (service, socket) = LspService::new(server::RephactorLanguageServer::new);
        Server::new(stdin, stdout, socket).serve(service).await;
    });
}
