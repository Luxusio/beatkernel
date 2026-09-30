//! Explicit read-only ASIO driver registration discovery; no driver is opened.
use std::{env, error::Error};

const HELP: &str =
    "Usage: asio_inspector --view native|32|64 [--max-drivers N] [--max-value-units N]
       asio_inspector --help
Windows only: enumerate ASIO registrations in the selected registry view.
Defaults: max-drivers 256; max-value-units 4096 (UTF-16 including terminal NUL).
Registration does not establish driver loadability or audio output support.";

struct Options {
    view: String,
    max_drivers: usize,
    max_value_units: usize,
}

fn parse(args: &[String]) -> Result<Options, Box<dyn Error>> {
    let mut view = None;
    let mut max_drivers = 256;
    let mut max_value_units = 4096;
    let mut seen = [false; 3];
    let mut index = 0;
    while index < args.len() {
        let slot = match args[index].as_str() {
            "--view" => 0,
            "--max-drivers" => 1,
            "--max-value-units" => 2,
            other => return Err(format!("unknown option {other:?}; use --help").into()),
        };
        if seen[slot] {
            return Err(format!("duplicate option {:?}", args[index]).into());
        }
        seen[slot] = true;
        let value = args.get(index + 1).ok_or("missing option value")?;
        match slot {
            0 => {
                if !matches!(value.as_str(), "native" | "32" | "64") {
                    return Err("--view must be native, 32 or 64".into());
                }
                view = Some(value.clone());
            }
            1 => {
                max_drivers = value.parse()?;
                if !(1..=4096).contains(&max_drivers) {
                    return Err("--max-drivers must be 1..4096".into());
                }
            }
            _ => {
                max_value_units = value.parse()?;
                if !(2..=32768).contains(&max_value_units) {
                    return Err("--max-value-units must be 2..32768".into());
                }
            }
        }
        index += 2;
    }
    Ok(Options {
        view: view.ok_or("--view is required; use --help")?,
        max_drivers,
        max_value_units,
    })
}

#[cfg(target_os = "windows")]
fn inspect(options: Options) -> Result<(), Box<dyn Error>> {
    use beatkernel_platform::windows::asio::{
        enumerate_asio_drivers, AsioEnumerationLimits, AsioRegistryView,
    };
    let view = match options.view.as_str() {
        "native" => AsioRegistryView::Native,
        "32" => AsioRegistryView::Bits32,
        "64" => AsioRegistryView::Bits64,
        _ => unreachable!("validated view"),
    };
    let drivers = enumerate_asio_drivers(
        view,
        AsioEnumerationLimits {
            max_drivers: options.max_drivers,
            max_value_units: options.max_value_units,
        },
    )?;
    println!("registered_drivers={} view={view:?}", drivers.len());
    for driver in drivers {
        println!(
            "name={:?} description={:?} clsid={} view={:?}",
            driver.name, driver.description, driver.id.clsid, driver.id.view
        );
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn inspect(options: Options) -> Result<(), Box<dyn Error>> {
    let _ = (options.view, options.max_drivers, options.max_value_units);
    Err("ASIO registry discovery requires Windows".into())
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().skip(1).collect();
    if args.as_slice() == ["--help"] {
        println!("{HELP}");
        return Ok(());
    }
    inspect(parse(&args)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Result<Options, Box<dyn Error>> {
        parse(
            &args
                .iter()
                .map(|value| (*value).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn view_and_count_caps_are_explicit_without_driver_access() {
        for view in ["native", "32", "64"] {
            let parsed = options(&["--view", view]).unwrap();
            assert_eq!(parsed.view, view);
            assert_eq!(parsed.max_drivers, 256);
            assert_eq!(parsed.max_value_units, 4096);
        }
        let parsed = options(&[
            "--max-drivers",
            "4096",
            "--view",
            "64",
            "--max-value-units",
            "32768",
        ])
        .unwrap();
        assert_eq!((parsed.max_drivers, parsed.max_value_units), (4096, 32768));
    }

    #[test]
    fn malformed_requests_never_reach_registry_access() {
        for args in [
            vec![],
            vec!["--view"],
            vec!["--view", "auto"],
            vec!["--view", "64", "--view", "32"],
            vec!["--list"],
            vec!["--view", "native", "--max-drivers", "0"],
            vec!["--view", "native", "--max-drivers", "4097"],
            vec!["--view", "native", "--max-value-units", "1"],
            vec!["--view", "native", "--max-value-units", "32769"],
            vec!["--view", "native", "--max-value-units", "-2"],
            vec![
                "--view",
                "native",
                "--max-value-units",
                "184467440737095516160",
            ],
        ] {
            assert!(options(&args).is_err(), "{args:?}");
        }
    }
}
