use clap::Parser;
use owo_colors::OwoColorize;
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    path::{Path, PathBuf},
};
use toml_edit::{value, Document, Table};
use walkdir::{DirEntry, WalkDir};

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Addional Cargo.toml file to include
    ///
    /// Add multiple values using --include /path/foo --include /path/bar
    #[clap(short, long)]
    include_for_dep_updates: Vec<String>,

    /// Exclude packages from being updated
    ///
    /// Add multiple values using --exclude foo --exclude bar
    #[clap(short, long)]
    exclude_from_version_change: Vec<String>,

    /// Maximum depth of directory traversal
    #[clap(short, long, default_value_t = 3)]
    max_depth: usize,

    /// Version to be used in all updates
    #[clap(short, long, required = true)]
    update_version: String,
}

const IGNORE_DIRS: &'static [&'static str] = &[".git", "target"];

// Always return full path
fn fullpath(test_path: String) -> PathBuf {
    let mut path = PathBuf::new();
    path.push(test_path.clone());

    if path.is_absolute() {
        path
    } else {
        let current_dir = env::current_dir().expect("Could not get current directory!");
        path.push(current_dir);
        path.push(test_path.clone());
        path
    }
}

fn is_not_excluded_dir(entry: &DirEntry) -> bool {
    let metadata = entry.metadata().expect(
        format!(
            "Could not get metadata for: {}",
            entry.file_name().to_str().unwrap()
        )
        .as_str(),
    );

    if metadata.is_dir() {
        return !IGNORE_DIRS.contains(&entry.file_name().to_str().unwrap());
    }

    true
}

fn is_cargo_toml_file(entry: &DirEntry) -> bool {
    let metadata = entry.metadata().expect(
        format!(
            "Could not get metadata for: {}",
            entry.file_name().to_str().unwrap()
        )
        .as_str(),
    );

    if metadata.is_file() {
        if entry.file_name().eq_ignore_ascii_case("Cargo.toml") {}
        return entry.file_name().eq("Cargo.toml");
    }

    false
}

fn parse_new_version(old_version_specifier: &str, new_version: &str) -> String {
    let mut result = String::new();

    if old_version_specifier.chars().nth(0).unwrap().is_numeric() {
        result.push_str(old_version_specifier);
    } else {
        let version_pos = old_version_specifier
            .find(|c: char| c.is_numeric())
            .unwrap();

        result.push_str(&old_version_specifier[..version_pos]);
        result.push_str(&new_version.clone());
    }

    result
}

fn main() {
    let args = Args::parse();
    let current_dir = env::current_dir().expect("Could not get current directory!");

    let mut deps_update_files_set: HashSet<String> = HashSet::new();
    // for file in args.include_for_dep_updates {
    //     deps_update_files_set.insert(fullpath(file).to_str().unwrap().to_string());
    // }

    // println!("deps_update_files_set: {:?}", deps_update_files_set);

    let mut exclude_version_files_set: HashSet<String> = HashSet::new();
    for file in args.exclude_from_version_change {
        exclude_version_files_set.insert(fullpath(file).to_str().unwrap().to_string());
    }

    // println!("exclude_version_files_set: {:?}", exclude_version_files_set);

    let mut updatable_package_names_set: HashSet<String> = HashSet::new();
    let mut files_to_process_set: HashSet<String> = HashSet::new();

    for entry in WalkDir::new(&current_dir)
        .into_iter()
        .filter_entry(|e| is_not_excluded_dir(e))
        .filter_map(|v| v.ok())
    {
        if is_cargo_toml_file(&entry) {
            let filepath: String = fullpath(entry.path().display().to_string())
                .display()
                .to_string();

            let mut output = format!(
                "{} Cargo.toml file at {}",
                "Discovered".bold().green(),
                &filepath.cyan()
            );

            if exclude_version_files_set.contains(&filepath) {
                output.push_str(
                    "\n           * Excluding from package version bump due to command line parameter"
                        .dimmed()
                        .to_string()
                        .as_str(),
                )
            } else {
                let data = fs::read_to_string(&filepath)
                    .expect(format!("Unable to open file at {}", &filepath).as_str());
                let doc = data.parse::<Document>().expect(
                    format!(
                        "File at location {} is an invalid Cargo.toml file",
                        &filepath
                    )
                    .as_str(),
                );

                if doc.contains_key("package") {
                    let package_table = doc.get("package").unwrap().as_table().unwrap();

                    if package_table.contains_key("name") {
                        let package_name = package_table
                            .get("name")
                            .unwrap()
                            .as_str()
                            .unwrap()
                            .to_string();

                        updatable_package_names_set.insert(package_name);
                    }
                } else {
                    output.push_str(
                        "\n           * Could not determine package name due to [package] not existing -- skipping version bump."
                            .dimmed()
                            .to_string()
                            .as_str(),
                    )
                }
            }

            files_to_process_set.insert(filepath.clone());
            // println!("{output}");
        }
    }

    for file in args.include_for_dep_updates {
        let filepath: String = fullpath(file).display().to_string();

        let mut output = format!(
            "{} Cargo.toml file at {} for processing",
            " Including".bold().green(),
            &filepath.cyan()
        );

        if exclude_version_files_set.contains(&filepath) {
            output.push_str(
                "\n           * Excluding from package version bump due to command line parameter"
                    .dimmed()
                    .to_string()
                    .as_str(),
            )
        } else {
            let data = fs::read_to_string(&filepath)
                .expect(format!("Unable to open file at {}", &filepath).as_str());
            let doc = data.parse::<Document>().expect(
                format!(
                    "File at location {} is an invalid Cargo.toml file",
                    &filepath
                )
                .as_str(),
            );

            if doc.contains_key("package") {
                let package_table = doc.get("package").unwrap().as_table().unwrap();

                if package_table.contains_key("name") {
                    let package_name = package_table
                        .get("name")
                        .unwrap()
                        .as_str()
                        .unwrap()
                        .to_string();

                    updatable_package_names_set.insert(package_name);
                }
            } else {
                output.push_str(
                        "\n          * Could not determine package name due to [package] not existing -- skipping version bump."
                            .dimmed()
                            .to_string()
                            .as_str(),
                    )
            }
        }

        println!("{output}");
        deps_update_files_set.insert(filepath.clone());
    }

    for package_name in &updatable_package_names_set {
        println!(
            "{} {} found for version updating",
            "   Package".bold().green(),
            package_name.cyan()
        );
    }

    for filepath in files_to_process_set.union(&deps_update_files_set) {
        println!(
            "{} Cargo.toml file at {}",
            "Processing".bold().green(),
            &filepath.cyan()
        );

        let data = fs::read_to_string(&filepath)
            .expect(format!("Unable to open file at {}", &filepath).as_str());

        let mut doc = data.parse::<Document>().expect(
            format!(
                "File at location {} is an invalid Cargo.toml file",
                &filepath
            )
            .as_str(),
        );

        if !exclude_version_files_set.contains(filepath) {
            if doc.contains_key("package") {
                doc["package"]["version"] = value(args.update_version.clone());
            }
        }

        for updatable_table in vec!["dependencies", "build-dependencies", "dev-dependencies"] {
            if doc.contains_table(updatable_table) {
                let mut deps_table: &mut Table =
                    // doc.get_mut("dependencies").unwrap().as_table_mut().unwrap();
                    doc.get_mut(updatable_table).unwrap().as_table_mut().unwrap();

                for package in &updatable_package_names_set {
                    // if deps_table.contains_table(package) {
                    //     let inner_table =
                    //         deps_table.get_mut(package).unwrap().as_table_mut().unwrap();

                    //     let old_version = inner_table.get("version").unwrap();
                    //     let new_version = parse_new_version(
                    //         old_version.as_str().unwrap(),
                    //         &args.update_version.as_str(),
                    //     );
                    //     inner_table["version"] = value(new_version);
                    // }

                    if deps_table.contains_key(package) {
                        let dep_value = deps_table.get_mut(package).unwrap();

                        if dep_value.is_table() {
                            let old_version = dep_value.get("version").unwrap();
                            let new_version = parse_new_version(
                                old_version.as_str().unwrap(),
                                &args.update_version.as_str(),
                            );
                            dep_value["version"] = value(new_version);
                        } else if dep_value.is_inline_table() {
                            let inline_table = dep_value.as_inline_table().unwrap();

                            if inline_table.contains_key("version") {
                                let old_version = inline_table.get("version").unwrap();
                                let new_version = parse_new_version(
                                    old_version.as_str().unwrap(),
                                    &args.update_version.as_str(),
                                );
                                deps_table[package]["version"] = value(new_version);
                            }
                        } else {
                            let new_version = parse_new_version(
                                dep_value.as_str().unwrap(),
                                &args.update_version.as_str(),
                            );

                            deps_table[package] = value(new_version);
                        }
                    }
                }
            }
        }

        /*
                if doc.contains_table("dependencies") {
                    let mut deps_table: &mut Table =
                        doc.get_mut("dependencies").unwrap().as_table_mut().unwrap();

                    for package in &updatable_package_names_set {
                        // if deps.contains_key(format!("dependencies.{package}").as_str()) {
                        //     println!("=====================================================================================");
                        // }
                        // if deps.contains_key(format!("dependencies.{}", package).as_str()) {
                        // println!(">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>");
                        // } else if deps.contains_key(package) {

                        if deps_table.contains_table(package) {
                            // println!("========================================");
                            let inner_table = deps_table.get_mut(package).unwrap().as_table_mut().unwrap();

                            // println!("GGGGGGGGGGGGGGGGGGGGGG: {:?}", g);
                            let old_version = inner_table.get("version").unwrap();
                            let new_version = parse_new_version(
                                old_version.as_str().unwrap(),
                                &args.update_version.as_str(),
                            );
                            inner_table["version"] = value(new_version);
                        }

                        if deps_table.contains_key(package) {
                            let dep_value = deps_table.get(package).unwrap();

                            if dep_value.is_table() {
                                let table = dep_value.as_table().unwrap();
                                // println!(">>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>>  {} is normal table: {:#?}\n", package, table);
                                // // let new_version = parse_new_version(
                                // //     dep_value.as_str().unwrap(),
                                // //     &args.update_version.as_str(),
                                // // );
                                // // deps[package] = value(new_version);
                                // let table_name = format!("dependencies.{}", package);

                                // deps[table_name.as_str()].as_table_mut().unwrap()["version"] =
                                //     value("adsfsaf");

                                // deps["dependencies.dsjkfljkas"][package]["version"] =
                                //     value("+++++++++++++++++++++++++++++++++++++++++++++++");
                                // table["version"] = value("dfjsaklfdjaskfjdkljfkl");
                            } else if dep_value.is_inline_table() {
                                // println!("is inline table: {:?}\n", dep_value);
                                let inline_table = dep_value.as_inline_table().unwrap();

                                if inline_table.contains_key("version") {
                                    let old_version = inline_table.get("version").unwrap();
                                    let new_version = parse_new_version(
                                        old_version.as_str().unwrap(),
                                        &args.update_version.as_str(),
                                    );
                                    deps_table[package]["version"] = value(new_version);
                                }
                                // pgx-pg-config= { path = "../pgx-pg-config/", version = "=0.5.0-beta.1" }
                                // pgx-utils = { path = "../pgx-utils/", version = "=0.5.0-beta.1" }
                            } else {
                                // println!("is normal string: {:?}\n", dep_value);
                                let new_version = parse_new_version(
                                    dep_value.as_str().unwrap(),
                                    &args.update_version.as_str(),
                                );

                                deps_table[package] = value(new_version);
                            }
                        }
                    }
                }
        */
        println!("doc: {}", doc);
        fs::write(filepath, doc.to_string()).expect("Unable to write file");
    }
}
