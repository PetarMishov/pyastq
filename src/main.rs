mod find;
mod query_parser;

use find::find_op;

fn functions(_directory: &str) {
    todo!()
}

fn classes(_directory: &str) {
    todo!()
}

fn imports(_directory: &str, _file: &str) {
    todo!()
}

fn who_imports(_directory: &str, _file: &str) {
    todo!()
}

fn summary(_directory: &str) {
    todo!()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        [_, "find", directory, pattern] => {
            if let Err(error) = find_op(directory, pattern) {
                eprintln!("error: {error}");
                std::process::exit(2);
            }
        }
        [_, "functions", directory] => functions(directory),
        [_, "classes", directory] => classes(directory),
        [_, "imports", directory, file] => imports(directory, file),
        [_, "who-imports", directory, file] => who_imports(directory, file),
        [_, "summary", directory] => summary(directory),
        _ => {
            eprintln!("Usage:");
            eprintln!("  past find <directory> <pattern>");
            eprintln!("  past functions <directory>");
            eprintln!("  past classes <directory>");
            eprintln!("  past imports <directory> <file>");
            eprintln!("  past who-imports <directory> <file>");
            eprintln!("  past summary <directory>");
        }
    }
}
