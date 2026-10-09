//! Generate source-derived seeds into an exclusively new directory.
//! Parents must remain caller-controlled while this command runs; standard
//! filesystem APIs do not defend against concurrent hostile parent renames.
//! Errors may leave partial output, which is diagnosed and never overwritten.

use beatkernel_layer_fuzz::seeds::seed_cases;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("seed corpus failed: {error}; partial output may remain; choose a new output directory before retrying");
        std::process::exit(1);
    }
}

fn run() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    let output = args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: seed_corpus OUTPUT_DIRECTORY",
        )
    })?;
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected exactly one output directory",
        ));
    }
    let root = Path::new(&output);
    refuse_symlink_components(root)?;
    // create_dir, rather than create_dir_all, gives exclusive admission and
    // requires the caller to supply an existing, controlled parent.
    fs::create_dir(root)?;
    for target in [
        "input_codec",
        "replay_codec",
        "room_codec",
        "bms_parser",
        "chart_compiler",
    ] {
        fs::create_dir(root.join(target))?;
    }
    let cases = seed_cases();
    for (index, (target, bytes)) in cases.iter().enumerate() {
        let directory = root.join(target);
        refuse_symlink_components(&directory)?;
        let destination = directory.join(format!("seed-{index:03}"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)?;
        file.write_all(bytes)?;
    }
    println!("wrote {} seeds to {}", cases.len(), root.display());
    Ok(())
}

fn refuse_symlink_components(path: &Path) -> io::Result<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    };
    let mut prefix = std::path::PathBuf::new();
    for component in absolute.components() {
        if component == Component::ParentDir {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "parent-directory components are not accepted",
            ));
        }
        prefix.push(component);
        match fs::symlink_metadata(&prefix) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "refusing symlink destination component: {}",
                        prefix.display()
                    ),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
