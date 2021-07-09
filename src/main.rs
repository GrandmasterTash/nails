///
/// This is the main binary entry-point. The main gumf is in a lib.rs file as that allows our
/// integration tests to create the app with the same initialisation code as this binary.
///
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    nails::lib_main().await
}