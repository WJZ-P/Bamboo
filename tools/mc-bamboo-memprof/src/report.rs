//! 把一份快照按归属汇总成表。
//!
//! Groups one snapshot by owner.

use std::{collections::HashMap, error::Error, fs};

use crate::owners;

/// 一个分配点：活跃字节、活跃块数，和从内到外的函数名（内联的已经展开）。
///
/// One site: live bytes, live blocks, and function names innermost first with
/// inlined callees expanded.
struct Site {
    bytes: u64,
    blocks: u64,
    names: Vec<String>,
}

fn load_symbols(path: &str) -> Result<HashMap<String, Vec<String>>, Box<dyn Error>> {
    let mut symbols = HashMap::new();
    for line in fs::read_to_string(path)?.lines() {
        let (address, names) = line.split_once('\t').unwrap_or((line, ""));
        let names = names
            .split(" | ")
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        symbols.insert(address.to_owned(), names);
    }
    Ok(symbols)
}

fn load_sites(
    path: &str,
    symbols: &HashMap<String, Vec<String>>,
) -> Result<Vec<Site>, Box<dyn Error>> {
    let mut sites = Vec::new();
    for line in fs::read_to_string(path)?.lines() {
        let mut fields = line.split(' ');
        let bytes = fields.next().ok_or("missing live bytes")?.parse()?;
        let blocks = fields.next().ok_or("missing live blocks")?.parse()?;
        let names = fields
            .skip(2)
            .flat_map(|address| symbols.get(address).cloned().unwrap_or_default())
            .collect();
        sites.push(Site {
            bytes,
            blocks,
            names,
        });
    }
    Ok(sites)
}

/// 标准库、容器和分析器自己的栈帧：显示调用栈时跳过。
///
/// Frames of std, containers and the profiler itself, skipped when printing.
fn is_plumbing(name: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "alloc::",
        "core::",
        "std::",
        "hashbrown::",
        "mc_bamboo_memprof::tracker",
        "__rust",
        "__rustc",
        "serde::",
        "serde_core::",
        "enum2$<",
        "tuple$<",
    ];
    let name = name.trim_start_matches("impl$<").trim_start_matches('<');
    PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

/// 最靠里的那个「自己的」函数，作为没有命中任何规则时的分组键。
///
/// The innermost non-plumbing function: the grouping key of last resort.
fn leaf(site: &Site) -> &str {
    site.names
        .iter()
        .find(|name| !is_plumbing(name))
        .map_or("(no symbol)", String::as_str)
}

fn stack(site: &Site, limit: usize) -> String {
    let mut shown: Vec<&str> = Vec::new();
    for name in site.names.iter().filter(|name| !is_plumbing(name)) {
        if shown.last() != Some(&name.as_str()) {
            shown.push(name);
        }
    }
    shown.truncate(limit);
    shown.join("\n        <- ")
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn print_groups<'a>(title: &str, sites: impl Iterator<Item = (&'a str, &'a Site)>, top: usize) {
    let mut groups: HashMap<&str, (u64, u64)> = HashMap::new();
    for (key, site) in sites {
        let group = groups.entry(key).or_default();
        group.0 += site.bytes;
        group.1 += site.blocks;
    }
    let mut rows: Vec<_> = groups.into_iter().collect();
    rows.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.0.cmp(b.0)));
    let total: u64 = rows.iter().map(|row| row.1.0).sum();
    println!("== {title}: {:.1} MB", mb(total));
    println!("{:>10} {:>10} {:>9}  name", "MB", "blocks", "avg B");
    for (name, (bytes, blocks)) in rows.into_iter().take(top) {
        let average = bytes.checked_div(blocks).unwrap_or(0);
        println!("{:>10.2} {blocks:>10} {average:>9}  {name}", mb(bytes));
    }
}

pub fn run(args: &[String]) -> Result<(), Box<dyn Error>> {
    let [raw, symbols, ..] = args else {
        return Err("usage: bamboo-memprof report <snapshot.raw> <symbols.txt> [options]".into());
    };
    let number = |name: &str, default: usize| {
        crate::option(args, name)
            .and_then(|value| value.parse().ok())
            .unwrap_or(default)
    };
    let top = number("--top", 40);
    let stacks = number("--stacks", 0);
    let only = crate::option(args, "--owner");
    let symbols = load_symbols(symbols)?;
    let mut sites = load_sites(raw, &symbols)?;
    sites.sort_by_key(|site| std::cmp::Reverse(site.bytes));
    let owned: Vec<(&str, &Site)> = sites
        .iter()
        .map(|site| (owners::owner_of(&site.names), site))
        .filter(|(owner, _)| only.as_deref().is_none_or(|only| only == *owner))
        .collect();
    print_groups("live heap by owner", owned.iter().copied(), usize::MAX);
    println!();
    print_groups(
        "live heap by innermost function",
        owned.iter().map(|(_, site)| (leaf(site), *site)),
        top,
    );
    for (owner, site) in owned.iter().take(stacks) {
        println!(
            "\n{:.2} MB in {} blocks [{owner}]\n        {}",
            mb(site.bytes),
            site.blocks,
            stack(site, 14)
        );
    }
    Ok(())
}
