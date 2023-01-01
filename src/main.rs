use clap::Parser;
use console::Term;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Confirm;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use sqlx::migrate::MigrateDatabase;
use std::path::PathBuf;
use std::time::Duration;

mod core;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[arg(help = "Path to the HealthKit export.zip data")]
    export_zip: PathBuf,
    #[arg(help = "URL to the SQLite database", env = "DATABASE_URL")]
    db_url: String,
    #[arg(
        help = "Prompts the user to drop the database if it already exists",
        short,
        long
    )]
    drop: bool,
    #[arg(help = "Responds yes to all prompts", short, long)]
    yes: bool,
    #[arg(help = "Minimize stdout output", short, long)]
    quiet: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    env_logger::init();

    let cli = Cli::parse();
    let term = Term::stdout();

    // Abort the program if the database already exists and the user didn't specify the --force flag
    let database_uri = &cli.db_url;
    if sqlx::Sqlite::database_exists(database_uri).await? {
        let drop_prompt = format!("The database at \"{}\" already exists. Do you want to drop it? This will delete all data in the database.", database_uri);
        if cli.drop
            && (cli.yes
                || Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt(drop_prompt)
                    .default(false)
                    .interact()
                    .unwrap())
        {
            if !cli.quiet {
                term.write_line(&format!("Dropping database at \"{}\"...", database_uri))?;
            }
            sqlx::Sqlite::drop_database(database_uri).await?;
        } else {
            term.write_line(&format!(
                "The database at \"{}\" already exists. Please delete it or specify a different database URL.",
                database_uri)
            )?;
            return Ok(());
        }
    }

    let pb = ProgressBar::new_spinner();
    if cli.quiet {
        pb.set_draw_target(ProgressDrawTarget::hidden());
    }
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_style(
        ProgressStyle::with_template("[{elapsed_precise}] {spinner:.blue} {msg}")
            .unwrap()
            .tick_strings(&[
                "▹▹▹▹▹",
                "▸▹▹▹▹",
                "▹▸▹▹▹",
                "▹▹▸▹▹",
                "▹▹▹▸▹",
                "▹▹▹▹▸",
                "▪▪▪▪▪",
            ]),
    );
    pb.set_message(format!(
        "Creating SQLite database \"{}\" from \"{}\"...",
        cli.db_url,
        cli.export_zip.display(),
    ));

    core::healthkit_to_sqlite(database_uri, &cli.export_zip).await?;
    pb.finish_with_message(format!("Created SQLite database {}", cli.db_url));
    Ok(())
}
