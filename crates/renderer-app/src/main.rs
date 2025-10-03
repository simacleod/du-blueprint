use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    let mut initial_path: Option<PathBuf> = None;

    while let Some(arg) = args.next() {
        if arg == "--path" {
            if let Some(value) = args.next() {
                initial_path = Some(PathBuf::from(value));
            }
        }
    }

    if let Err(err) = du_blueprint_renderer::run(initial_path) {
        eprintln!("{}", err);
        std::process::exit(1);
    }
}
