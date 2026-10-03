use std::path::PathBuf;
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mut case = None;
    let mut corpus = None;
    let mut write_cases = None;
    let mut count = 20;
    let mut seed = 0x5eed_u64;
    let mut directory = PathBuf::from("lab-log/seam-failures");
    let mut args = args.iter();
    while let Some(key) = args.next() {
        let value = args.next().expect("seam check: option needs a value");
        match key.as_str() {
            "--case" => case = Some(PathBuf::from(value)),
            "--corpus" => corpus = Some(PathBuf::from(value)),
            "--write-cases" => write_cases = Some(PathBuf::from(value)),
            "--count" => count = value.parse().expect("seam check: count"),
            "--seed" => seed = value.parse().expect("seam check: decimal u64 seed"),
            "--failure-dir" => directory = value.into(),
            _ => panic!("seam check: unknown option {key}"),
        }
    }
    assert!(
        case.is_none() || corpus.is_none(),
        "choose --case or --corpus"
    );
    let cases = if let Some(path) = case {
        vec![void_seam_check::Case::read(path)]
    } else if let Some(path) = corpus {
        let mut files: Vec<_> = std::fs::read_dir(path)
            .expect("seam corpus: read directory")
            .map(|entry| entry.expect("seam corpus: directory entry").path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "seam corpus: no JSON cases");
        files.iter().map(void_seam_check::Case::read).collect()
    } else {
        void_seam_check::cases(seed, count)
    };
    if let Some(path) = write_cases {
        std::fs::create_dir_all(&path).expect("seam corpus: create directory");
        for case in &cases {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join(format!("case-{:016x}-{}.json", case.seed, case.index)))
                .expect("seam corpus: create new case");
            file.write_all(&serde_json::to_vec_pretty(case).unwrap())
                .expect("seam corpus: write");
            file.sync_all().expect("seam corpus: sync");
        }
    }
    let mut failed = 0;
    for case in &cases {
        match void_seam_check::run_case(case, &directory) {
            Ok(metrics) => println!("case {}: {}", case.index, metrics),
            Err(reason) => {
                eprintln!("case {} FAILED: {reason}", case.index);
                failed += 1;
            }
        }
    }
    println!(
        "seam checks: {} passed, {failed} failed",
        cases.len() - failed
    );
    if failed > 0 {
        std::process::exit(1);
    }
}
