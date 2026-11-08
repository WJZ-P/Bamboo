//! 官方 `Util.parseAndValidateUntrustedUri` 和 Bamboo 的白名单对照。
//!
//! Bamboo 收下的必须是官方收下的子集。官方多收的可以，Bamboo 多收的不行。
//!
//! Compares Bamboo's whitelist with official `Util.parseAndValidateUntrustedUri`.
//! Every URI Bamboo accepts must be one official code accepts. Official code may
//! accept more.

use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use mc_bamboo_protocol::components::uri::accepts;

#[test]
fn bamboo_urls_are_a_subset_of_the_official_ones() {
    let cases = cases();
    let official = official(&cases);
    let mut bamboo_yes = 0usize;
    let mut official_yes = 0usize;
    let mut only_bamboo = Vec::new();
    for (uri, official_ok) in cases.iter().zip(official) {
        let bamboo_ok = accepts(uri);
        bamboo_yes += usize::from(bamboo_ok);
        official_yes += usize::from(official_ok);
        if bamboo_ok && !official_ok {
            only_bamboo.push(uri.clone());
        }
    }
    println!(
        "uri oracle: cases {}, bamboo {}, official {}, bamboo-only {}",
        cases.len(),
        bamboo_yes,
        official_yes,
        only_bamboo.len()
    );
    assert!(
        only_bamboo.is_empty(),
        "Bamboo accepted URIs official code rejects: {only_bamboo:?}"
    );
}

fn official(cases: &[String]) -> Vec<bool> {
    let mut child = Command::new(java())
        .arg("-cp")
        .arg(classpath())
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/UriOracle.java"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("java");
    let mut stdin = child.stdin.take().expect("stdin");
    for uri in cases {
        writeln!(stdin, "{}", base64(uri.as_bytes())).expect("write");
    }
    drop(stdin);
    let stdout = child.stdout.take().expect("stdout");
    let answers: Vec<bool> = BufReader::new(stdout)
        .lines()
        .map(|line| line.expect("line") == "1")
        .collect();
    let status = child.wait().expect("wait");
    assert!(status.success(), "java exited {status}");
    assert_eq!(answers.len(), cases.len());
    answers
}

fn cases() -> Vec<String> {
    let mut set = BTreeSet::new();
    for uri in BOUNDARIES {
        set.insert((*uri).to_owned());
    }
    let mut rng = Lcg(0x5eed_0c0ffee);
    for _ in 0..1500 {
        set.insert(random_url(&mut rng));
    }
    for _ in 0..1500 {
        set.insert(random_bytes(&mut rng));
    }
    set.into_iter().collect()
}

const BOUNDARIES: &[&str] = &[
    "",
    "http",
    "http:",
    "http:/",
    "http://",
    "http://.",
    "http://a",
    "http://a.",
    "http://.a",
    "http://-a.com",
    "http://a-.com",
    "http://a--b.com",
    "HTTP://Example.COM",
    "https://example.com",
    "HtTpS://example.com/a",
    "http://example.com:0",
    "http://example.com:80",
    "http://example.com:00080",
    "http://example.com:65535",
    "http://example.com:65536",
    "http://example.com:99999",
    "http://example.com:",
    "http://example.com:80:80",
    "http://user@example.com",
    "http://user:pass@example.com",
    "http://1.2.3.4",
    "http://0.0.0.0",
    "http://01.2.3.4",
    "http://1.2.3.256",
    "http://1.2.3",
    "http://1.2.3.4.5",
    "http://[::]",
    "http://[::1]",
    "http://[1::]",
    "http://[1:2:3:4:5:6:7:8]",
    "http://[1:2:3:4:5:6:7:8:9]",
    "http://[1:2:3:4:5:6:7::]",
    "http://[1:2:3:4:5:6:7:8::]",
    "http://[1::2::3]",
    "http://[gggg::]",
    "http://[::ffff:1.2.3.4]",
    "http://[2001:DB8::1]",
    "http://[::1]:8080/",
    "http://[::1",
    "http://[fe80::1%eth0]/",
    "http://[v1.fe80::1]/",
    "http:foo",
    "http://example.com/a/b?x=1&y=2#frag",
    "http://example.com?x=1",
    "http://example.com#frag",
    "http://example.com?",
    "http://example.com#",
    "http://example.com/%2e%2e/",
    "http://example.com/%",
    "http://example.com/%2",
    "http://example.com/%2G",
    "http://example.com/%20",
    "http://example.com/a b",
    "http://example.com/a[b]",
    "http://ex_ample.com",
    "http://example.com.",
    "http://example..com",
    "http://例子.com",
    "http://example.com/😀",
    "ftp://example.com",
    "javascript:alert(1)",
    " http://example.com",
    "http://example.com ",
    "http://example.com/a\nb",
    "http://exam ple.com",
    "http://example.com\\path",
    "http://127.0.0.1:80/",
    "https://[2001:db8::1]/a?b=c",
];

fn random_url(rng: &mut Lcg) -> String {
    let scheme = ["http", "https", "HTTP", "ftp", "http:"][rng.index(5)];
    let host = match rng.index(6) {
        0 => "example.com".to_owned(),
        1 => "127.0.0.1".to_owned(),
        2 => "[::1]".to_owned(),
        3 => format!("{}.{}.invalid", label(rng), label(rng)),
        4 => format!(
            "{}.{}.{}.{}",
            rng.index(300),
            rng.index(300),
            rng.index(300),
            rng.index(300)
        ),
        _ => format!("{}:{}", label(rng), rng.index(70_000)),
    };
    let path = if rng.index(3) == 0 {
        String::new()
    } else {
        format!("/{}", token(rng))
    };
    format!("{scheme}://{host}{path}?{}#{}", token(rng), token(rng))
}

fn label(rng: &mut Lcg) -> String {
    let alphabet = b"abcXYZ019-._% ";
    let len = 1 + rng.index(8);
    (0..len)
        .map(|_| alphabet[rng.index(alphabet.len())] as char)
        .collect()
}

fn token(rng: &mut Lcg) -> String {
    let alphabet = b"abcXYZ019-._~%2G/?#[]@ ";
    let len = rng.index(10);
    (0..len)
        .map(|_| alphabet[rng.index(alphabet.len())] as char)
        .collect()
}

fn random_bytes(rng: &mut Lcg) -> String {
    let alphabet = "abcXYZ019-._~:/?#[]@% \n\u{7f}\u{4e2d}😀";
    let chars: Vec<char> = alphabet.chars().collect();
    let len = rng.index(24);
    (0..len).map(|_| chars[rng.index(chars.len())]).collect()
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }

    fn index(&mut self, n: usize) -> usize {
        (self.next() >> 33) as usize % n
    }
}

fn java() -> PathBuf {
    let known = PathBuf::from(r"C:\Users\Administrator\.jdks\openjdk-26.0.1\bin\java.exe");
    if known.exists() {
        return known;
    }
    PathBuf::from("java")
}

fn classpath() -> std::ffi::OsString {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/vanilla");
    let mut jars = Vec::new();
    collect(&root.join("libraries"), &mut jars);
    collect(&root.join("versions"), &mut jars);
    assert!(!jars.is_empty(), "target/vanilla has no jars");
    std::env::join_paths(jars).expect("classpath")
}

fn collect(dir: &Path, jars: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, jars);
        } else if path.extension().is_some_and(|ext| ext == "jar") {
            jars.push(path);
        }
    }
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let n = u32::from(bytes[index]) << 16
            | u32::from(bytes[index + 1]) << 8
            | u32::from(bytes[index + 2]);
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push(TABLE[(n & 63) as usize] as char);
        index += 3;
    }
    let rest = bytes.len() - index;
    if rest == 1 {
        let n = u32::from(bytes[index]) << 16;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = u32::from(bytes[index]) << 16 | u32::from(bytes[index + 1]) << 8;
        out.push(TABLE[((n >> 18) & 63) as usize] as char);
        out.push(TABLE[((n >> 12) & 63) as usize] as char);
        out.push(TABLE[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}
