mod cache;
mod cli;
mod files;
mod query;
mod report;
mod rules;
mod search;
#[cfg(test)]
mod search_tests;

fn main() {
    std::process::exit(cli::run());
}
