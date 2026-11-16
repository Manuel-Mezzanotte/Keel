//! The ports that programs started in Keel's terminals listen on, such as
//! 3000 for `pnpm run dev`, read with `ps` and `lsof`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::process::Command;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListeningPort {
    pub port: u16,
    pub pid: u32,
    /// The program, such as `node`.
    pub process: String,
    /// Where it listens: `*`, `127.0.0.1`, `[::1]`…
    pub address: String,
}

/// The TCP ports the given processes, or the processes they started,
/// listen on, one entry per port.
pub fn listening_ports(roots: &[u32]) -> Vec<ListeningPort> {
    if roots.is_empty() {
        return Vec::new();
    }
    let Some(table) = run("ps", &["-axo", "pid=,ppid="]) else {
        return Vec::new();
    };
    let pids = descendants(&parse_ps(&table), roots);
    let list = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    // `-a` keeps only what matches all the filters; without it `-p` and
    // `-i` would add up.
    let Some(output) = run(
        "lsof",
        &["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-p", &list, "-Fpcn"],
    ) else {
        return Vec::new();
    };
    parse_lsof(&output)
}

fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    // lsof exits with 1 when nothing listens.
    Some(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `pid ppid` lines into a map from each process to its parent.
fn parse_ps(table: &str) -> HashMap<u32, u32> {
    table
        .lines()
        .filter_map(|line| {
            let mut numbers = line.split_whitespace().map(str::parse::<u32>);
            Some((numbers.next()?.ok()?, numbers.next()?.ok()?))
        })
        .collect()
}

/// The roots and every process below them.
fn descendants(parents: &HashMap<u32, u32>, roots: &[u32]) -> Vec<u32> {
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for (&pid, &parent) in parents {
        children.entry(parent).or_default().push(pid);
    }
    let mut found: HashSet<u32> = HashSet::new();
    let mut stack = roots.to_vec();
    while let Some(pid) = stack.pop() {
        if found.insert(pid)
            && let Some(below) = children.get(&pid)
        {
            stack.extend(below);
        }
    }
    let mut found: Vec<u32> = found.into_iter().collect();
    found.sort_unstable();
    found
}

/// lsof's field output: `p` starts a process, `c` names it, `n` is an
/// address such as `*:3000` or `[::1]:5173`.
fn parse_lsof(output: &str) -> Vec<ListeningPort> {
    let mut ports: BTreeMap<u16, ListeningPort> = BTreeMap::new();
    let mut pid = 0;
    let mut process = String::new();
    for line in output.lines() {
        let Some(kind) = line.chars().next() else {
            continue;
        };
        let value = &line[kind.len_utf8()..];
        match kind {
            'p' => {
                pid = value.parse().unwrap_or(0);
                process.clear();
            }
            'c' => process = value.to_owned(),
            'n' => {
                let Some((address, port)) = value.rsplit_once(':') else {
                    continue;
                };
                let Ok(port) = port.parse() else {
                    continue;
                };
                ports.entry(port).or_insert_with(|| ListeningPort {
                    port,
                    pid,
                    process: process.clone(),
                    address: address.to_owned(),
                });
            }
            _ => {}
        }
    }
    ports.into_values().collect()
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn reads_lsof_fields() {
        let output =
            "p812\ncnode\nf23\nn*:3000\nf24\nn[::1]:3000\np900\ncpython3\nf3\nn127.0.0.1:8000\n";
        assert_eq!(
            parse_lsof(output),
            [
                ListeningPort {
                    port: 3000,
                    pid: 812,
                    process: "node".into(),
                    address: "*".into()
                },
                ListeningPort {
                    port: 8000,
                    pid: 900,
                    process: "python3".into(),
                    address: "127.0.0.1".into()
                },
            ]
        );
    }

    #[test]
    fn follows_children() {
        let parents = parse_ps("  1 0\n 10 1\n 11 10\n 12 11\n 20 1\n");
        assert_eq!(descendants(&parents, &[10]), [10, 11, 12]);
    }

    /// How long one look takes; run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn timing() {
        let _listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let start = std::time::Instant::now();
        for _ in 0..10 {
            listening_ports(&[std::process::id()]);
        }
        eprintln!("listening_ports: {:?} each", start.elapsed() / 10);
    }

    #[test]
    fn finds_a_real_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let ports = listening_ports(&[std::process::id()]);
        assert!(
            ports.iter().any(|found| found.port == port),
            "{port} not in {ports:?}"
        );
    }
}
