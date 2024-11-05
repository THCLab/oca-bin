use clap::ArgGroup;

#[derive(clap::Args, Clone, Debug)]
#[clap(
    group = ArgGroup::new("summary_group")
        .required(false)
        .multiple(false)
        .args(&["summary", "no_summary", "summary_json"])
)]
pub struct SummaryGroup {
    /// Print summary in JSON
    #[clap(long, group = "summary_group", action)]
    summary_json: bool,
    /// Print summary in human readable format
    #[clap(long, group = "summary_group", action)]
    summary: bool,
    /// Do not print any summary
    #[clap(long, group = "summary_group", action)]
    no_summary: bool,
}

#[derive(Debug)]
pub enum SummaryOptions {
    Human,
    Json,
    None,
}

impl SummaryGroup {
    pub fn summary(&self) -> SummaryOptions {
        match (self.summary_json, self.summary, self.no_summary) {
            (true, false, false) => SummaryOptions::Json,
            (false, true, false) => SummaryOptions::Human,
            (false, false, true) => SummaryOptions::None,
            (false, false, false) => SummaryOptions::Human,
            _ => unreachable!(),
        }
    }
}
