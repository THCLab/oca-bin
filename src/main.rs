use crate::mapping::mapping;
use build::{changed_files, handle_cache, load_cache};
use build::CacheError;
use cache::{Cache, PathCache};
use config::create_or_open_local_storage;
use config::OCA_CACHE_DB_DIR;
use config::OCA_INDEX_DIR;
use config::OCA_REPOSITORY_DIR;
use dependency_graph::parse_name;
use dependency_graph::GraphError;
use error::CliError;
use itertools::Itertools;
use oca_presentation::presentation::Presentation;
use presentation_command::PresentationCommand;
use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::sync::Mutex;
use std::{env, fs, fs::File, io::Write, path::PathBuf, process, str::FromStr};
use tui::app::App;
use utils::handle_panic;
use utils::load_nodes;
use utils::load_remote_repo_url;
use utils::send_to_repo;
use utils::visit_dirs_recursive;

use clap::Parser as ClapParser;
use clap::Subcommand;
use oca_rs::{
    repositories::SQLiteConfig, Facade,
};
use url::Url;

use crate::config::{init_or_read_config, write_config, Config, OCA_DIR_NAME};
use crate::dependency_graph::parse_node;
use crate::dependency_graph::DependencyGraph;
use crate::dependency_graph::MutableGraph;
use crate::presentation_command::{handle_generate, handle_validate, Format};
use crate::tui::logging::initialize_logging;
use crate::utils::{load_ocafiles_all, visit_current_dir};
use said::SelfAddressingIdentifier;
use serde::{Deserialize, Serialize};

extern crate dirs;

#[macro_use]
extern crate log;

mod build;
mod config;
mod dependency_graph;
pub mod error;
mod mapping;
pub mod presentation_command;
mod tui;
mod utils;
mod validate;
mod cache;

#[derive(clap::Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,
    #[arg(short, long)]
    config_path: Option<String>,
}

#[derive(Subcommand)]
#[command(arg_required_else_help = true)]
enum Commands {
    /// Initialize new local repository
    Init {},
    /// Show configuration where data are stored
    Config {},
    /// Build oca objects out of ocafile
    #[clap(group = clap::ArgGroup::new("build").multiple(true).required(true).args(&["ocafile", "directory"]))]
    Build {
        /// Specify ocafile to build from
        #[arg(short = 'f', long, group = "build")]
        ocafile: Option<Vec<PathBuf>>,
        /// Build oca objects from directory (recursive)
        #[arg(short, long, group = "build")]
        directory: Option<PathBuf>,
        /// Publish build ocafiles
        #[clap(long, short, action)]
        publish: bool,
    },
    /// Validate oca objects out of ocafile
    #[clap(group = clap::ArgGroup::new("build").multiple(true).required(true).args(&["ocafile", "directory"]))]
    Validate {
        /// Specify ocafile to validate from
        #[arg(short = 'f', long, group = "build")]
        ocafile: Option<Vec<PathBuf>>,
        /// Validate oca objects from directory (recursive)
        #[arg(short, long, group = "build")]
        directory: Option<PathBuf>,
    },
    /// Publish oca objects into online repository
    #[clap(group = clap::ArgGroup::new("publish").required(true).multiple(false).args(&["said", "directory","dirty"]))]
    Publish {
        #[arg(short, long)]
        repository_url: Option<String>,
        #[arg(short, long, group = "publish")]
        said: Option<String>,
        #[arg(long, group = "publish", action)]
        dirty: bool,
        #[arg(short, long, group = "publish")]
        directory: Option<PathBuf>,
        #[arg(short, long)]
        timeout: Option<u64>,
    },
    /// Show ocafile for specify said
    Show {
        #[arg(short, long)]
        said: String,
        #[arg(short, long)]
        ast: bool,
        #[arg(short, long)]
        dereference: bool,
    },
    /// Get oca bundle for specify said
    Get {
        #[arg(short, long)]
        said: String,
        #[arg(short, long)]
        with_dependencies: bool,
    },
    /// List of all oca objects stored in local repository
    List {},
    /// Generate or parse presentation for oca object
    Presentation {
        #[command(subcommand)]
        command: PresentationCommand,
    },
    /// Launches a terminal user interface application to browse OCA objects
    Tui {
        /// Browse oca objects from directory (recursive)
        #[arg(short, long)]
        dir: Option<PathBuf>,
        /// Publishing timeout in seconds. Default is 30.
        #[arg(short, long)]
        timeout: Option<u64>,
    },
    /// Generate json file with all fields of oca object for specified said
    Mapping {
        #[arg(short, long)]
        said: String,
    },
    /// Returns list of oca objects that uses provided ocafile as dependency
    Deps {
        /// Specify ocafile
        #[arg(short = 'f', long)]
        ocafile: PathBuf,
        /// Directory that contains dependent oca files (recursive)
        #[arg(short, long)]
        directory: PathBuf,
    },
}

fn get_oca_facade(local_repository_path: PathBuf) -> Facade {
    let db = create_or_open_local_storage(local_repository_path.join(OCA_REPOSITORY_DIR));
    let cache = create_or_open_local_storage(local_repository_path.join(OCA_CACHE_DB_DIR));
    let cache_storage_config = SQLiteConfig::build()
        .path(local_repository_path.join(OCA_INDEX_DIR))
        .unwrap();
    Facade::new(Box::new(db.clone()), Box::new(cache), cache_storage_config)
}

fn saids_to_publish(
    facade: Arc<Mutex<Facade>>,
    saids: &[SelfAddressingIdentifier],
) -> HashSet<SelfAddressingIdentifier> {
    let mut to_publish = HashSet::new();
    for said in saids {
        to_publish.insert(said.clone());
        let dep_said = dependant_saids(facade.clone(), said);
        if let Some(deps) = dep_said {
            let more_deps = saids_to_publish(facade.clone(), &deps);
            to_publish.extend(more_deps);
        };
    }
    to_publish
}

fn dependant_saids(
    facade: Arc<Mutex<Facade>>,
    said: &SelfAddressingIdentifier,
) -> Option<Vec<SelfAddressingIdentifier>> {
    let facade_locked = facade.lock().unwrap();
    let bundles = facade_locked.get_oca_bundle(said.clone(), true).unwrap();
    let saids = bundles.dependencies;

    if saids.is_empty() {
        None
    } else {
        Some(
            saids
                .iter()
                .map(|bdle| bdle.said.as_ref().unwrap().clone())
                .collect::<Vec<_>>(),
        )
    }
}

/// Publish oca bundle pointed by SAID to configured repository
///
/// # Arguments
/// * `said` - SAID of oca bundle to publish
///
///
fn publish_oca_file_for(
    facade: Arc<Mutex<Facade>>,
    said: SelfAddressingIdentifier,
    timeout: &Option<u64>,
    repository_url: Url,
) -> Result<(), CliError> {
    let timeout = timeout.unwrap_or(666);
    let facade = facade.lock().unwrap();

    match facade.get_oca_bundle_ocafile(said.clone(), false) {
        Ok(ocafile) => send_to_repo(&repository_url, ocafile, timeout),
        Err(errors) => Err(CliError::PublishError(said, errors)),
    }
}

fn main() -> Result<(), CliError> {
    initialize_logging().unwrap();

    App::setup_panic_hooks()?;
    let args = Args::parse();

    // Any command triggered
    // Check if config exist if not ask user to initialize the repo
    // or read the config to properly parse all stuff
    // first check if there is local .oca
    // then check home dir for global .oca
    // default is one repo for all in home dir
    //

    let config = init_or_read_config();
    info!("Config: {:?}", config);
    let local_repository_path = config.local_repository_path;
    let remote_repo_url_from_config = config.repository_url;

    let unwind_res = std::panic::catch_unwind(AssertUnwindSafe(|| {
        match &args.command {
            Some(Commands::Init {}) => {
                info!("Initialize local repository");
                match env::current_dir() {
                    Ok(path) => {
                        info!("Initialize repository at: {:?}", path);
                        let local_repository_path = path.join(OCA_DIR_NAME);
                        let config = Config::new(local_repository_path);
                        let config_file = path.join(OCA_DIR_NAME).join("config.toml");
                        match write_config(&config, &config_file) {
                            Ok(it) => Ok(it),
                            Err(err) => {
                                println!("{}", err);
                                Err(CliError::WriteFileFailed(err))
                            }
                        }
                    }
                    Err(err) => Err(CliError::CurrentDirFailed(err)),
                }
            }
            Some(Commands::Config {}) => {
                info!("Configuration of oca");
                println!(
                    "Local repository: {:?} ",
                    local_repository_path.join(OCA_REPOSITORY_DIR)
                );
                println!(
                    "OCA Cache: {:?} ",
                    local_repository_path.join(OCA_CACHE_DB_DIR)
                );
                println!("Index DB: {:?}", local_repository_path.join(OCA_INDEX_DIR));
                Ok(())
            }
            Some(Commands::Build {
                ocafile,
                directory,
                publish,
            }) => {
                let nodes = load_nodes(ocafile.clone(), directory.as_ref())?;
                
                let (cached_digests, cache_said, nodes_to_build) = match directory.as_ref() {
                    // Handle cache. Returns nodes that need to be updated.
                    Some(cache_path) => {
                        match handle_cache(&cache_path, &nodes) {
                            Ok((cache, cache2, nodes_to_update)) => {
                                let paths_to_rebuild = nodes_to_update
                                    .iter()
                                    .map(|node| node.path.to_str().unwrap())
                                    .join("\n\t•");
                                if !paths_to_rebuild.is_empty() {
                                    println!(
                                        "The following files will be rebuilt: \n\t• {}",
                                        paths_to_rebuild
                                    );
                                };
                            
                                (Some(cache), Some(cache2), nodes_to_update)
                            },
                            Err(CacheError::NoChanges) => {
                                println!("Up to date");
                                return Ok(());
                            },
                            Err(e) => return Err(e.into()),
                        }
                    }
                    None => (None, None, nodes)
                };

                // Handle build
                let mut facade = get_oca_facade(local_repository_path);
                let mut oca_files_to_publish = Vec::new();
                for node in nodes_to_build.iter() {
                    let mut to_publish = build::build(&mut facade, node, cache_said.as_ref(), cached_digests.as_ref())?;
                    oca_files_to_publish.append(&mut to_publish);
                }

                // Handle publish 
                if *publish {
                    let remote_repo_url = load_remote_repo_url(&None, remote_repo_url_from_config)?;
                    println!("Publishing to {}", &remote_repo_url);
                    for to_publish in oca_files_to_publish {
                        send_to_repo(&remote_repo_url, to_publish, 666)?;
                    }
                };

                cache_said.map(|c| c.save().unwrap());
                cached_digests.map(|c| c.save().unwrap());
               
                Ok(())
            }

            Some(Commands::Publish {
                repository_url,
                said,
                timeout,
                dirty,
                directory,
            }) => match (said, directory, dirty) {
                (Some(said), None, false) => {
                    info!("Publish OCA bundle to repository");
                    let facade = Arc::new(Mutex::new(get_oca_facade(local_repository_path)));
                    match SelfAddressingIdentifier::from_str(said) {
                        Ok(said) => {
                            let remote_repo_url =
                                load_remote_repo_url(repository_url, remote_repo_url_from_config)?;

                            publish_oca_file_for(
                                facade.clone(),
                                said.clone(),
                                timeout,
                                remote_repo_url.clone(),
                            )?;
                            Ok(())
                        }
                        Err(err) => {
                            println!("Invalid SAID: {}", err);
                            Err(err.into())
                        }
                    }
                }
                (None, Some(directory), false) => {
                    let mut cache_path = directory.clone();
                    cache_path.push(".oca-bin");
                    // let _ = File::create(&cache_path);
                    let cache: PathCache = Cache::new(cache_path.clone());
                    // let cache = load_cache(&cache_path).unwrap_or_default();
                    let all_paths = visit_dirs_recursive(directory)?;

                    // Detect edited files, that weren't built yet
                    let changes = changed_files(all_paths.iter(), &cache);
                    if !changes.is_empty() {
                        println!("There are changes in following files, that wasn't build yet: ");
                        println!("\t•{}", changes.into_iter().map(|path| path.to_str().unwrap()).join("\n\t• "));
                        println!("They won't be published.");
                    }
                    let mut said_cache_path = directory.clone();
                    said_cache_path.push(".oca-saids");
                    println!("Loading saids cache: {:?}", &said_cache_path);
                    let said_cache = Cache::new(said_cache_path);

                    let facade = Arc::new(Mutex::new(get_oca_facade(local_repository_path)));

                    // Cache for saving already published saids, to avoid publishing dependant saids multiple times.
                    let mut local_cache = vec![];
                    // Iter through built elements and publish them
                    let _: Vec<()> = all_paths
                        .into_iter()
                        .map(|path| {
                            // Version of file while it was built.
                            let file_digest = cache.get(&path).unwrap();
                            match file_digest {
                                Some(digest) => {
                                    // Built said
                                    let said = said_cache.get(&digest).unwrap();

                                    match said {
                                        Some(said) => {
                                            // Get saids that provided said
                                            // depends on. Elements need to be
                                            // published in proper order, for
                                            // oca-repo to be able to process
                                            // them.
                                            for said in saids_to_publish(facade.clone(), &[said]) {
                                                if !local_cache.contains(&said) {
                                                    println!("Publish OCA bundle: {} to repository. File path: {:?}", &said, &path);
                                                    // publish_oca_file_for(
                                                    //     facade.clone(),
                                                    //     said.clone(),
                                                    //     timeout,
                                                    //     remote_repo_url.clone(),
                                                    // ).unwrap();
                                                    local_cache.push(said);
                                                };
                                            }
                                        },
                                        None => {
                                            // Cache error. OCA bundle said not cached.
                                            todo!("No cached said for file hash: {} of file: {:?}.", &digest, &path)
                                        },
                                    }
                                },
                                None => {
                                    // New file, not built yet.
                                    println!("New unbuild ocafile file: {:?}. Won't be published", &path)
                                }
                            }
                            
                        })
                        .collect();
                        Ok(())
                    
                },
                (None, None, true) => {
                    // publish built ocafiles that weren't publish
                    todo!()
                },
                _ => todo!()
            },
            Some(Commands::List {}) => {
                info!(
                    "List OCA object from local repository: {:?}",
                    local_repository_path
                );
                let facade = get_oca_facade(local_repository_path);
                let mut page = 1;
                let page_size = 20;
                let mut result = facade.fetch_all_oca_bundle(page_size, page).unwrap();
                let meta = result.metadata;
                let mut count = 0;
                info!("Found {} objects in repo", meta.total);
                let refs = facade.fetch_all_refs().unwrap();
                info!("Found {:#?} refs in repo", refs);
                loop {
                    info!("Processing page: {}, count: {}", page, count);
                    if count >= meta.total {
                        break;
                    }
                    let records = result.records;
                    count += records.len();
                    info!("Processing {} objects", records.len());
                    for bundle in records {
                        let said = bundle.said.unwrap();
                        let matching_ref = refs.iter().find(|&(_, v)| *v == said.to_string());
                        match matching_ref {
                            Some((refs, _)) => {
                                println!("SAID: {}, name: {}", said, refs);
                            }
                            None => {
                                println!("SAID: {}", said);
                            }
                        }
                    }
                    page += 1;
                    result = facade.fetch_all_oca_bundle(page_size, page).unwrap();
                }
                Ok(())
            }
            Some(Commands::Show {
                said,
                ast,
                dereference,
            }) => {
                info!("Search for OCA object in local repository");
                let facade = get_oca_facade(local_repository_path);
                match SelfAddressingIdentifier::from_str(said) {
                    Ok(said) => {
                        if *ast {
                            let oca_ast = facade
                                .get_oca_bundle_ast(said)
                                .map_err(CliError::OcaBundleAstError)?;
                            serde_json::to_writer_pretty(std::io::stdout(), &oca_ast)
                                .expect("Faild to format oca ast");
                            Ok(())
                        } else {
                            let ocafile = facade
                                .get_oca_bundle_ocafile(said, *dereference)
                                .map_err(CliError::OcaBundleAstError)?;
                            println!("{}", ocafile);
                            Ok(())
                        }
                    }
                    Err(err) => {
                        println!("Invalid SAID: {}", err);
                        Err(CliError::InvalidSaid(err))
                    }
                }
            }
            Some(Commands::Get {
                said,
                with_dependencies,
            }) => {
                let facade = get_oca_facade(local_repository_path);
                let said = SelfAddressingIdentifier::from_str(said)?;
                let oca_bundles = facade
                    .get_oca_bundle(said, *with_dependencies)
                    .map_err(CliError::OcaBundleAstError)?;
                let content = serde_json::to_value(oca_bundles).map_err(CliError::ReadOcaError)?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&content).map_err(CliError::WriteOcaError)?
                );
                Ok(())
            }
            Some(Commands::Presentation { command }) => {
                match command {
                    PresentationCommand::Generate { said, format } => {
                        let said = SelfAddressingIdentifier::from_str(said)?;
                        let facade = get_oca_facade(local_repository_path);
                        let presentation = handle_generate(said, &facade)?;
                        let wrapped_presentation = WrappedPresentation { presentation };
                        let output = match format {
                            Some(f) => f.format(&wrapped_presentation),
                            None => Format::JSON.format(&wrapped_presentation),
                        };
                        println!("{}", output);
                        Ok(())
                    }
                    PresentationCommand::Validate {
                        from_file,
                        output,
                        format,
                        recalculate,
                    } => {
                        let ext = from_file.extension();
                        let extension = match ext {
                            Some(ext) => match ext.to_str() {
                                Some(ext) => Format::from_str(ext)
                                    .map_err(|e| CliError::FileExtensionError(e.to_string())),
                                None => Err(CliError::FileExtensionError(
                                    "Unsupported file extension".to_string(),
                                )),
                            },
                            None => {
                                warn!("Missing input file extension. Using JSON");
                                Ok(Format::JSON)
                            }
                        }?;

                        let file_contents = fs::read_to_string(from_file)
                            .map_err(|e| CliError::ReadFileFailed(from_file.clone(), e))?;
                        let pres: WrappedPresentation = match extension {
                            Format::JSON => serde_json::from_str(&file_contents).unwrap(),
                            Format::YAML => serde_yaml::from_str(&file_contents).unwrap(),
                        };
                        let pres = handle_validate(pres.presentation, *recalculate);
                        match pres {
                            Ok(pres) => {
                                let presentation_wrapped =
                                    WrappedPresentation { presentation: pres };
                                // save to file
                                let (path, content) = match (output, format) {
                                    (None, None) => {
                                        (from_file.into(), extension.format(&presentation_wrapped))
                                    }
                                    (None, Some(format)) => match format {
                                        Format::JSON => {
                                            let mut output_path = from_file.clone();
                                            output_path.set_extension("json");
                                            (
                                                output_path,
                                                serde_json::to_string_pretty(&presentation_wrapped)
                                                    .unwrap(),
                                            )
                                        }
                                        Format::YAML => {
                                            let mut output_path = from_file.clone();
                                            output_path.set_extension("yaml");
                                            (
                                                output_path,
                                                serde_yaml::to_string(&presentation_wrapped)
                                                    .unwrap(),
                                            )
                                        }
                                    },
                                    (Some(out), None) => {
                                        (out.into(), extension.format(&presentation_wrapped))
                                    }
                                    (Some(out), Some(format)) => {
                                        (out.into(), format.format(&presentation_wrapped))
                                    }
                                };

                                let mut file =
                                    File::create(path).map_err(CliError::WriteFileFailed)?;

                                file.write_all(content.as_bytes())
                                    .map_err(CliError::WriteFileFailed)?;
                                println!("Presentation SAID is valid");
                            }
                            Err(e) => {
                                println!("Error: {}", &e.to_string());
                            }
                        };
                        Ok(())
                    }
                }
            }
            Some(Commands::Validate { ocafile, directory }) => {
                let paths = match (ocafile, directory) {
                    (None, None) => unreachable!("At least one argument expected"),
                    (_, Some(dir)) => visit_dirs_recursive(dir)?,
                    (Some(oca_file), None) => oca_file.clone(),
                };

                let facade = get_oca_facade(local_repository_path);
                let facade = Arc::new(Mutex::new(facade));
                let mut graph = MutableGraph::new(paths)?;
                match ocafile {
                    Some(oca_file) => {
                        let mut cache = HashSet::new();
                        for file in oca_file {
                            // Insert ocafile to graph, if not present
                            let (node, dependencies) =
                                parse_node(file).map_err(|e| CliError::GraphError(e.into()))?;
                            match graph.insert_node(node.clone(), dependencies) {
                                Ok(_) => (),
                                // node already in graph
                                Err(GraphError::DuplicateKey {
                                    refn: _,
                                    first_path: _,
                                    second_path: _,
                                }) => (),
                                Err(e) => return Err(e.into()),
                            };
                            println!("Validating {}", &node.refn);
                            let (out_cache, errs) = validate::validate_directory(
                                facade.clone(),
                                &mut graph,
                                Some(node.refn),
                                &cache,
                            )?;
                            cache.extend(out_cache);
                            for err in errs {
                                println!("{}", err)
                            }
                        }
                    }
                    None => {
                        let (_cache, errs) = validate::validate_directory(
                            facade,
                            &mut graph,
                            None,
                            &HashSet::new(),
                        )?;

                        for err in errs {
                            println!("{}", err)
                        }
                    }
                };

                Ok(())
            }
            Some(Commands::Tui { dir, timeout }) => {
                if let Some(directory) = dir.as_ref() {
                    let all_oca_files =
                        load_ocafiles_all(None, Some(directory)).unwrap_or_else(|err| {
                            eprintln!("{err}");
                            process::exit(1);
                        });
                    let facade = Arc::new(Mutex::new(get_oca_facade(local_repository_path)));

                    let to_show = visit_current_dir(directory)?
                        .into_iter()
                        .map(|of| parse_node(&of).map(|(node, _)| node));
                    tui::draw(
                        directory.clone(),
                        to_show,
                        all_oca_files,
                        facade,
                        remote_repo_url_from_config,
                        *timeout,
                    )
                    .unwrap_or_else(|err| {
                        eprintln!("{err}");
                        process::exit(1);
                    });
                    Ok(())
                } else {
                    eprintln!("Specify the base working directory where you keep your ocafiles, e.g., oca tui -d ./my-ocafiles-repo");
                    process::exit(1);
                }
            }
            Some(Commands::Mapping { said }) => {
                let said = SelfAddressingIdentifier::from_str(said)?;
                let paths = load_ocafiles_all(None, Some(&local_repository_path))?;

                let facade = get_oca_facade(local_repository_path);

                let graph = DependencyGraph::from_paths(paths).unwrap();

                let o = mapping(said, &facade, &graph).unwrap();

                let actual_json = serde_json::to_string_pretty(&o).unwrap();
                println!("{}", actual_json);
                Ok(())
            }
            Some(Commands::Deps { ocafile, directory }) => {
                let paths = visit_dirs_recursive(directory)?;
                let graph = MutableGraph::new(paths)?;
                let (name, _) =
                    parse_name(ocafile).map_err(|_e| CliError::MissingRefn(ocafile.clone()))?;
                let out = graph
                    .get_ancestors([name.unwrap().as_str()], false)
                    .map_err(CliError::GraphError)?;
                for item in out {
                    println!("name: {}, path: {}", item.refn, item.path.to_str().unwrap());
                }
                Ok(())
            }
            None => Ok(()),
        }
    }));
    match unwind_res {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(er)) => {
            eprintln!("Error: {}", er);
            std::process::exit(1);
        }
        Err(panic) => {
            eprintln!("{}", handle_panic(panic));
            std::process::exit(1);
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct WrappedPresentation {
    presentation: Presentation,
}

// ocafile build -i OCAfile
// ocafile build -s scid
// ocafile publish
// ocafile fetch SAI
// ocafile inspect
