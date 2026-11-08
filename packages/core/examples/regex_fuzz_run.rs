use mkv_core::regex::Regex;
use std::io::Read;

fn unhex(s: &str) -> String {
    let bytes: Vec<u8> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect();
    String::from_utf8(bytes).expect("utf-8")
}

fn hexs(s: &str) -> String {
    s.as_bytes().iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    for line in input.lines() {
        let mut parts = line.split('\t');
        let pat = unhex(parts.next().unwrap_or(""));
        let text = unhex(parts.next().unwrap_or(""));
        let re = match Regex::new(&pat) {
            Ok(r) => r,
            Err(_) => {
                println!("ERR\t{pat}\t{text}");
                continue;
            }
        };
        let hit = re.find_at(&text, 0);
        let hs = match hit {
            Some(h) => format!("{},{}", h.start, h.end),
            None => "-".to_string(),
        };
        let allm: Vec<(usize, usize)> = re
            .find_all(&text, 0)
            .into_iter()
            .map(|x| (x.start, x.end))
            .collect();
        let tot: usize = allm.iter().map(|(_, e)| e).sum();
        println!("{}\t{}\t{hs}\t{allm:?}\t{tot}", hexs(&pat), hexs(&text));
    }
}
