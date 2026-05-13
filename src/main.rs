use tower_lsp::{LspService, Server};

mod document;
mod php;
mod server;

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(server::RephactorLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
