use clap::ArgGroup;


#[derive(clap::Args, Clone, Debug)]
#[clap(
    group = ArgGroup::new("summary_group")
        // .required(false)
        .multiple(false)
        .args(&["summary", "no_summary", "summary_json"])
)]
pub struct Summary {
    #[clap(long, group = "summary_group", action)]
    summary_json: bool,
    #[clap(long, group = "summary_group", action)]
    summary: bool,
    #[clap(long, group = "summary_group", action)]
    no_summary: bool,
}
