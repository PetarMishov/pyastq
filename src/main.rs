mod cache;
mod cli;
mod files;
mod query;
mod report;
mod rewrite;
mod rules;
mod search;

fn main() {
    std::process::exit(cli::run());
}
