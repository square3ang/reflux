//! Composer spellcheck (`Spellcheck.ts`, `SpellcheckDictionaries.ts`): Fluxer's Hunspell
//! dictionaries, downloaded on demand from its static CDN, checked against their SHA-256 and
//! cached, read by `spellbook` (pure Rust). Misspellings get a red underline in the composer,
//! and its context menu offers suggestions and "Add to dictionary".
// "Detect while typing" (`autoDetect`, the default) guesses the draft's language like the desktop
// app's `franc` (trigrams over the bundled languages, via `whatlang`) and checks against it plus
// the system and interface languages' dictionaries; off, the ones picked in Language settings.
// shortcut: no OS spellchecker engine.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};

use sha2::{Digest, Sha256};

const BASE_URL: &str = "https://fluxerstatic.com/desktop/spellcheck/dictionaries";
/// `BUILTIN_ALLOWLIST`.
const ALLOW: &[&str] = &["Fluxer", "fluxer", "Hampus"];

/// A `BUNDLED_DICTIONARIES` entry with its `DICTIONARY_SOURCES` hashes: (sha256, bytes).
pub struct Dict {
    pub package: &'static str,
    pub tag: &'static str,
    pub name: &'static str,
    version: &'static str,
    aff: (&'static str, usize),
    dic: (&'static str, usize),
}

pub const DICTS: &[Dict] = &[
    Dict { package: "dictionary-bg", tag: "bg", name: "Български", version: "2.0.0", aff: ("cb6bfc56d6b1e70c812061086d7ec5ad6aaf6d35acd927a035f1cee0188ea477", 58189), dic: ("44408b1ee50301e8f9a133b0ec92565bb8be49818c444d91821876d447f58906", 1566242) },
    Dict { package: "dictionary-ca", tag: "ca", name: "Català", version: "3.0.0", aff: ("bfc7d8412c9450ec63e00d13f2153191a11d04868b14d51cf6c50eace71efdb2", 49922), dic: ("e272aeff4b2d1a92b428a22b52c685292d04936e34dfe7786e27539175cbef2e", 2876608) },
    Dict { package: "dictionary-cs", tag: "cs", name: "Čeština", version: "4.0.0", aff: ("175e02acb4786042a9c87e3cb6d3746e3a5b3079752118c2d4f0bb2c756ce72b", 112714), dic: ("bbc0a495671c29337279182799e171c173a21ae8968c89dd0fb7c713b40c7832", 3648134) },
    Dict { package: "dictionary-da", tag: "da", name: "Dansk", version: "6.0.0", aff: ("cb5ef704de0ab8575c883c97981d5d242260c7ebe47b28c15c205443aca15976", 81584), dic: ("9a12345e1881c96f18cc460dbd062e47509571d5f2a2a9bf041da169856dc19e", 3719300) },
    Dict { package: "dictionary-de", tag: "de", name: "Deutsch", version: "3.0.0", aff: ("57fdd1b16aac2131003c91e0cf2a488becb970382a402a9ce089307301cb3ef0", 19199), dic: ("b5c781a0cf6f285fb6b9b8ab02fbea104b987104a1efdda8a835837e89e3ec77", 1118194) },
    Dict { package: "dictionary-el", tag: "el", name: "Ελληνικά", version: "4.0.0", aff: ("5d2e3580af6d3523ac4470335a4b05968e59dddc58e1a7f2b260c81342e80063", 20058), dic: ("f08daefb302600beb1b345e4fd77f4ecf6617aa080a72efe6ae7eec0ad5b2ac7", 19421967) },
    Dict { package: "dictionary-en", tag: "en", name: "English (US)", version: "4.0.0", aff: ("8ae1f19d4840d957728ad90555d5a8dff6cc5c046279c95ff0c00fc0a0136c7b", 3086), dic: ("f0b1a234bd178bdd01875b2a392a9647f888b8fe879f79c52aae62c2759b3647", 551762) },
    Dict { package: "dictionary-en-gb", tag: "en-GB", name: "English (UK)", version: "3.0.0", aff: ("8ae1f19d4840d957728ad90555d5a8dff6cc5c046279c95ff0c00fc0a0136c7b", 3086), dic: ("869fe17ba4ee4b5401c60a666ee2d6a3dcc237f460b7294435df1cc6a799aa57", 552106) },
    Dict { package: "dictionary-eo", tag: "eo", name: "Esperanto", version: "2.0.0", aff: ("a668bb25baccb39a758692a94325c47195a70fe723fe9875d1b3a3b4545412a5", 16322), dic: ("75b09812254cd6b49c2849ed3b92f1c252117906f5a2ce281d25e1798c437ac1", 382137) },
    Dict { package: "dictionary-es", tag: "es", name: "Español", version: "4.0.0", aff: ("892039d59db11d747061b81c3a92a52d48ba0e055b5835af1943cbd3c2fba85b", 167135), dic: ("907f786a8ceb3456722b20ad91dd4dbe99c5c16e45015ea63155e36f26b06d2c", 706202) },
    Dict { package: "dictionary-et", tag: "et", name: "Eesti", version: "2.0.0", aff: ("f568a1003d9d49d377e87e35a4b712a243b4490ef0647992c85169d46ed948e3", 240016), dic: ("237ff31093ec03c7f992919b8e7c9a4d3df90cdef9859edab571ebff42c9f5e8", 4457441) },
    Dict { package: "dictionary-eu", tag: "eu", name: "Euskara", version: "4.0.0", aff: ("ba68948c642828c80e5b5fc7ea19876bd38ce6f874f4453c662395adca8fef2a", 2729393), dic: ("2f046ed070c11f9dc6f502577462bdc11a0966be50e0d9750823922a22d9e948", 2303418) },
    Dict { package: "dictionary-fa", tag: "fa", name: "فارسی", version: "2.0.0", aff: ("dba0980816354ad969aa46dda0c040eeebda4a9381be331e17eae8647a2a1e1c", 5440), dic: ("314f31e059994f26e8016c834b14dfd10f74834b3f623a6040265b8f2a9dddab", 2575990) },
    Dict { package: "dictionary-fo", tag: "fo", name: "Føroyskt", version: "3.0.0", aff: ("6e459e5a3346ee0b7ad3ddc98dd0a412226060c6e7be6e096cfdf039f877b491", 170213), dic: ("3b62983af6b5c093e2ec020ff35869cc3068167e7cc7f8cc9585456a6b1368db", 1716099) },
    Dict { package: "dictionary-fr", tag: "fr", name: "Français", version: "3.0.0", aff: ("05a735d34c912e4e381ff08ee7c747923ccf5cf9dca81d8467982fa1ca51c2b7", 199870), dic: ("984e933237bc1224a48f42828233be9b03228260ef67aa8e2bdddcd03a26230d", 1229133) },
    Dict { package: "dictionary-gl", tag: "gl", name: "Galego", version: "3.0.0", aff: ("e2abc4122548c5f9e914fbbbcbebad3486b1492ac352fef72b95be93e32a0617", 1159910), dic: ("ca5f8d735a13225c1dabab042ddac8c58fd5e820ffba807ba8024d0fa0a68170", 8636406) },
    Dict { package: "dictionary-he", tag: "he", name: "עברית", version: "2.0.0", aff: ("e115a0a5014d17a8e904f9d4a67d7992039b396c5fa031eb036a13bf63dfb037", 78883), dic: ("9bd95042a927e13ab422231e355caa8a7018456e7c49b889f2e6c62ddb5a0552", 5688111) },
    Dict { package: "dictionary-hr", tag: "hr", name: "Hrvatski", version: "2.0.0", aff: ("230a7f0ca65159ecf8f3eea4afd79f7436415e83129b1d6b2424892074d8194d", 96099), dic: ("fb436f3a758cd4faf9ef4cd5c2e6e2a1e3abe535ee183095a2f59fd18dad0731", 732589) },
    Dict { package: "dictionary-hu", tag: "hu", name: "Magyar", version: "3.0.0", aff: ("625a5fb8bf77cf1a4d73288883b8abeff4753f736579a35ec285088fb9098d2c", 807730), dic: ("b6bda85603bb293de53b6deed5043771b1a61958f76e09aa8058c26e563f63e1", 3114250) },
    Dict { package: "dictionary-is", tag: "is", name: "Íslenska", version: "2.0.0", aff: ("0168050c64a369c55b5aeb61f9dfe484d2aa7512d78bb3fa36779ea24b467c50", 309734), dic: ("8447594fd7eb1d6d43725c4ca49b00d3f00c4c4f3ac2f2b6dfe3f56eb211d280", 2454138) },
    Dict { package: "dictionary-it", tag: "it", name: "Italiano", version: "2.0.0", aff: ("5770cd3e16d494c045b4a9a4a9fcd7962577e642d0384a7129c020a12cdd2c79", 70020), dic: ("b1348fbdb6f441ea9dd7e33b2cfcb96ead39ccd5e48bf894972774cd5aa86abb", 1295754) },
    Dict { package: "dictionary-ko", tag: "ko", name: "한국어", version: "2.0.0", aff: ("7b8930ce1357691591a3c38afc9cba787ec6dce0e30d0438bd156c4b4c94dbdf", 11094418), dic: ("ea275b5fe256925a95f9192a173f7c3d3c2480fa748371c64b89d7e0cc531b1d", 2864525) },
    Dict { package: "dictionary-lt", tag: "lt", name: "Lietuvių", version: "2.0.0", aff: ("79c6f8304490d1c8b27ec93e509b2f635d9bb21d45a2d41c15162b18c8ff897c", 99714), dic: ("a751750b040623cf408cf1a3b6f6336141b55ac9519e54068dbeaa52eb395ccd", 1181184) },
    Dict { package: "dictionary-lv", tag: "lv", name: "Latviešu", version: "2.0.0", aff: ("a11f646c6905d6c96f3218ada4a6cf84129c9e6169720ac1a17f1707940a2890", 130475), dic: ("bf7d054e28bc5e44df19410f3ed9d5fd8d6a4d8c051676650388c4e8d65c3148", 1844831) },
    Dict { package: "dictionary-nb", tag: "nb", name: "Norsk Bokmål", version: "3.0.0", aff: ("515a1e5f32ffa555d253c827830420dc5bb1d9a59ea9f720729acb0874c6a8c3", 17381), dic: ("e97fdf7f441b3c46e1d0761a8ad25fa1883d0977d70ea8cb04751cb1c47fa10e", 5349724) },
    Dict { package: "dictionary-nl", tag: "nl", name: "Nederlands", version: "2.0.0", aff: ("b430d9cf5f4170d9e5072397927e1402b34aa877d1f4c8fbc00dab6370d87312", 46732), dic: ("be192fdc36efd39dde0fb9a5a113143e0a906782e81ed68a74a710edced378d0", 2489166) },
    Dict { package: "dictionary-pl", tag: "pl", name: "Polski", version: "2.0.0", aff: ("de7b54a8fa37562d58b481cf3d87a63e3dcc2d74021f6403eb0e4f5df9ecc225", 268824), dic: ("74b6155391025419967c26913541b55b36d58e80b1028037d4a0b8ddcb18d0b7", 4683579) },
    Dict { package: "dictionary-pt", tag: "pt", name: "Português", version: "4.0.0", aff: ("a9f3621b94eb4d6838474bf9982027758e77dcb248815f49409b1a4a477dbd27", 979789), dic: ("32e2edd83541d58613bcc83ef731a79aadf85ed57e977a16c6b5f711a7a36cee", 4477692) },
    Dict { package: "dictionary-ro", tag: "ro", name: "Română", version: "3.0.0", aff: ("3ee127a667bdec829faee546ae32033e8f10f7c63117f9b572ef03ad9df2b76b", 55040), dic: ("c26a9356f598a0ae89e7be650f6bdd9ba70acce66b41d7ab14c0c68639b6ed33", 2202394) },
    Dict { package: "dictionary-ru", tag: "ru", name: "Русский", version: "3.0.0", aff: ("38ce7d4af78e211e9bafe4bf7e3d6a2c420591136cb738ec6648f8fdf6524cd7", 71236), dic: ("f6047416a0204adbecf3a451b874ec8a97ee37e2cbc714466ef04d8dbcc0d6fc", 3473191) },
    Dict { package: "dictionary-sk", tag: "sk", name: "Slovenčina", version: "2.0.0", aff: ("a79dc992b849aa2355db38eea9c4722239993639d77bc81c9c1aa79f213e723c", 99298), dic: ("b6cdb6fb512c44881cc91329aea0b179840f5f9a3e6cb024c2ab8547f96f5d94", 3502147) },
    Dict { package: "dictionary-sl", tag: "sl", name: "Slovenščina", version: "3.0.0", aff: ("511043a4f9c64a966599e3d53fa1f4e9a172a87de4a7a0c2cf327098a3a605d1", 15054), dic: ("5bac615dbd301ddbefbdaafdae242a89e46605916d77958df22778e770a95b9e", 3084448) },
    Dict { package: "dictionary-sr", tag: "sr", name: "Српски", version: "4.0.0", aff: ("10e11edbd55742975f0720b26a2b6669187e9b91fabc5063ee81f0db2f73279f", 1296), dic: ("8bdbbe464f20a5a600ccad1ff0257883c42bb6170a55440d45b4b39e002f433a", 4447970) },
    Dict { package: "dictionary-sv", tag: "sv", name: "Svenska", version: "4.0.0", aff: ("ff8ddb979daa9f128e7489ece5edd666ada7af787933221543b7cecb2b3ed478", 18581), dic: ("540ff8d0d9b05aae66dd919625d3a0bd25026d1e157027e6602e5dd11a07b645", 2344199) },
    Dict { package: "dictionary-tr", tag: "tr", name: "Türkçe", version: "2.0.0", aff: ("6a390c79b054ad427cf9a6bf6de43463953cc1918e7f727e9dd4ebd4f9702f42", 240814), dic: ("dc1d8f7724f4aefd117274678de431e4ea1765f2e02e6983b6ca48cbf5a7fc7c", 9117192) },
    Dict { package: "dictionary-uk", tag: "uk", name: "Українська", version: "4.0.0", aff: ("cd22837ffdbf96d57f3f7c5702f37f9069eb480cb3078668b32e1c69b0549fc1", 203526), dic: ("38fc49e64b54f7154a045edfd6e51f47ed4cb1e33c013a0e6de942713a82e199", 8448957) },
    Dict { package: "dictionary-vi", tag: "vi", name: "Tiếng Việt", version: "3.0.0", aff: ("0d836ebf1d4f1e9964d931fbb76464b24037b71995c73784166a07c43e2a7ae5", 1081), dic: ("9c56725b43e66de33d28aac8e58124e38247c8b2be34a8469b3113169110256c", 39858) },
];

#[derive(Default)]
struct Checker {
    enabled: bool,
    /// Package names wanted now.
    wanted: Vec<&'static str>,
    loaded: Vec<(&'static str, Arc<spellbook::Dictionary>)>,
    loading: HashSet<&'static str>,
    words: HashSet<String>,
    /// Detect while typing: the preferred languages' packages, the draft's guessed one, and the
    /// text it was guessed from.
    auto: bool,
    preferred: Vec<&'static str>,
    detected: Option<&'static str>,
    detected_from: String,
    /// Bumped when what's checked changes, so cached results go stale.
    generation: u64,
}

static CHECKER: LazyLock<RwLock<Checker>> = LazyLock::new(Default::default);

/// The dictionary for an interface language: its own tag, else its base language's.
pub fn for_locale(code: &str) -> Option<&'static Dict> {
    let base = match code {
        "no" => "nb",
        "en-GB" => "en-GB",
        c => c.split('-').next().unwrap_or(c),
    };
    DICTS.iter().find(|d| d.tag == code).or_else(|| DICTS.iter().find(|d| d.tag == base))
}

/// `detectPreferredLanguages`: the system locale (`LANG`, …) then the interface language.
fn preferred_locales() -> Vec<String> {
    let os = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty() && v != "C" && v != "POSIX"));
    let os = os.map(|v| v.split(['.', '@']).next().unwrap_or_default().replace('_', "-"));
    os.into_iter().chain([crate::i18n::locale().to_string()]).collect()
}

/// `chooseBundledTagForLanguage`: English is en-GB when a preferred locale is, else US.
fn package_for(base: &str, preferred: &[String]) -> Option<&'static str> {
    if base == "en" {
        let gb = preferred.iter().any(|l| for_locale(l).is_some_and(|d| d.tag == "en-GB"));
        return Some(if gb { "dictionary-en-gb" } else { "dictionary-en" });
    }
    preferred.iter().filter_map(|l| for_locale(l)).find(|d| d.tag.split('-').next() == Some(base)).or_else(|| for_locale(base)).map(|d| d.package)
}

/// `ISO6393_TO_DICTIONARY_BASE`.
const ISO6393: &[(&str, &str)] = &[
    ("bul", "bg"), ("cat", "ca"), ("ces", "cs"), ("dan", "da"), ("deu", "de"), ("ell", "el"), ("eng", "en"), ("epo", "eo"), ("est", "et"), ("eus", "eu"),
    ("fas", "fa"), ("fra", "fr"), ("glg", "gl"), ("heb", "he"), ("hrv", "hr"), ("hun", "hu"), ("isl", "is"), ("ita", "it"), ("kor", "ko"), ("lav", "lv"),
    ("lit", "lt"), ("nld", "nl"), ("nob", "nb"), ("pol", "pl"), ("por", "pt"), ("ron", "ro"), ("rus", "ru"), ("slk", "sk"), ("slv", "sl"), ("spa", "es"),
    ("srp", "sr"), ("swe", "sv"), ("tur", "tr"), ("ukr", "uk"), ("vie", "vi"),
];

static DETECTOR: LazyLock<whatlang::Detector> = LazyLock::new(|| whatlang::Detector::with_allowlist(ISO6393.iter().filter_map(|(c, _)| whatlang::Lang::from_code(*c)).collect()));

/// `detectLanguageFromText`: the dictionary for the text's language (2000 characters at most,
/// three letters at least).
fn detect_package(text: &str) -> Option<&'static str> {
    let text: String = text.chars().take(2000).collect();
    if text.chars().filter(|c| c.is_alphabetic()).count() < 3 {
        return None;
    }
    let code = DETECTOR.detect_lang(&text)?.code();
    let base = ISO6393.iter().find(|(c, _)| *c == code)?.1;
    package_for(base, &preferred_locales())
}

/// The packages [`super::Prefs`] asks for (with detection, before any draft is read).
pub fn wanted(p: &super::Prefs) -> Vec<&'static str> {
    if p.spell_auto {
        let pref = preferred_locales();
        let mut out: Vec<&'static str> = Vec::new();
        for l in &pref {
            if let Some(d) = for_locale(l).and_then(|d| package_for(d.tag.split('-').next().unwrap_or(d.tag), &pref))
                && !out.contains(&d)
            {
                out.push(d);
            }
        }
        return out;
    }
    if p.spell_langs.is_empty() {
        return for_locale(crate::i18n::locale()).map(|d| d.package).into_iter().collect();
    }
    DICTS.iter().filter(|d| p.spell_langs.iter().any(|l| l == d.package)).map(|d| d.package).collect()
}

/// Applies the preferences and loads missing dictionaries in the background.
pub fn configure(p: &super::Prefs) {
    let mut c = CHECKER.write().unwrap();
    c.enabled = p.spellcheck;
    c.auto = p.spell_auto;
    c.preferred = wanted(p);
    c.wanted = match c.detected.filter(|_| c.auto) {
        Some(d) => std::iter::once(d).chain(c.preferred.iter().copied().filter(|x| *x != d)).collect(),
        None => c.preferred.clone(),
    };
    c.words = p.words.iter().cloned().collect();
    c.generation += 1;
    load_missing(&mut c);
}

/// Starts loading the wanted dictionaries not loaded yet.
fn load_missing(c: &mut Checker) {
    let missing: Vec<&'static str> = c.wanted.iter().copied().filter(|w| !c.loaded.iter().any(|(p, _)| p == w) && !c.loading.contains(w)).collect();
    if !c.enabled || rune::is_headless() {
        return;
    }
    for package in missing {
        let Some(d) = DICTS.iter().find(|d| d.package == package) else { continue };
        c.loading.insert(package);
        crate::app::spawn(async move {
            let r = load(d).await;
            let mut c = CHECKER.write().unwrap();
            c.loading.remove(package);
            match r {
                Ok(dict) => {
                    c.loaded.push((package, Arc::new(dict)));
                    c.generation += 1;
                    crate::app::update(|_| {}); // redraw with the underlines
                }
                Err(e) => eprintln!("reflux: spellcheck {package}: {e}"),
            }
        });
    }
}

fn cache_dir(d: &Dict) -> Option<PathBuf> {
    Some(dirs::cache_dir()?.join("reflux/spellcheck").join(format!("{}@{}", d.package, d.version)))
}

fn verify(data: &[u8], (sha, len): (&str, usize)) -> bool {
    data.len() == len && format!("{:x}", Sha256::digest(data)) == sha
}

/// One file: the cached copy if it verifies, else downloaded, verified and cached.
async fn file(d: &Dict, name: &str, want: (&str, usize)) -> Result<Vec<u8>, String> {
    let path = cache_dir(d).map(|p| p.join(name));
    if let Some(p) = &path
        && let Ok(b) = tokio::fs::read(p).await
        && verify(&b, want)
    {
        return Ok(b);
    }
    let url = format!("{BASE_URL}/{}@{}/{name}", d.package, d.version);
    let r = reflux_net::http_client().get(&url).send().await.map_err(|e| e.to_string())?.error_for_status().map_err(|e| e.to_string())?;
    let b = r.bytes().await.map_err(|e| e.to_string())?.to_vec();
    if !verify(&b, want) {
        return Err(format!("{name} failed verification"));
    }
    if let Some(p) = &path {
        let _ = tokio::fs::create_dir_all(p.parent().expect("cache dir")).await;
        let _ = tokio::fs::write(p, &b).await;
    }
    Ok(b)
}

async fn load(d: &'static Dict) -> Result<spellbook::Dictionary, String> {
    let aff = file(d, "index.aff", d.aff).await?;
    let dic = file(d, "index.dic", d.dic).await?;
    tokio::task::spawn_blocking(move || {
        let (aff, dic) = (String::from_utf8_lossy(&aff), String::from_utf8_lossy(&dic));
        spellbook::Dictionary::new(&aff, &dic).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Uses `dict` as `package` (the demo's and tests' inline dictionaries).
pub fn insert(package: &'static str, dict: spellbook::Dictionary) {
    let mut c = CHECKER.write().unwrap();
    (c.enabled, c.wanted) = (true, vec![package]);
    c.loaded.push((package, Arc::new(dict)));
    c.generation += 1;
}

/// The words of `text` worth checking: letters and inner apostrophes, outside links, mentions,
/// emoji shortcodes and code, and not the word at the caret (still being typed).
fn words(text: &str, caret: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut code = false;
    let mut pos = 0;
    for chunk in text.split_inclusive(char::is_whitespace) {
        let start = pos;
        pos += chunk.len();
        let ticks = chunk.matches('`').count();
        let skip = code || ticks > 0 || chunk.contains("://") || chunk.starts_with(['<', ':', '@', '#']) || chunk.chars().any(|c| c.is_ascii_digit());
        code ^= ticks % 2 == 1;
        if skip {
            continue;
        }
        let mut w: Option<usize> = None;
        for (i, c) in chunk.char_indices().chain([(chunk.len(), ' ')]) {
            let letter = c.is_alphabetic() || ((c == '\'' || c == '’') && w.is_some());
            match (letter, w) {
                (true, None) => w = Some(i),
                (false, Some(s)) => {
                    let word = chunk[s..i].trim_end_matches(['\'', '’']);
                    let r = start + s..start + s + word.len();
                    if word.chars().count() > 1 && !(r.start <= caret && caret <= r.end) {
                        out.push(r);
                    }
                    w = None;
                }
                _ => {}
            }
        }
    }
    out
}

fn correct(c: &Checker, word: &str) -> bool {
    ALLOW.contains(&word) || c.words.contains(word) || c.loaded.iter().filter(|(p, _)| c.wanted.contains(p)).any(|(_, d)| d.check(word))
}

/// Misspelled byte ranges; none while spellcheck is off or no dictionary has loaded.
pub fn misspelled(text: &str, caret: usize) -> Vec<Range<usize>> {
    let c = CHECKER.read().unwrap();
    if !c.enabled || !c.loaded.iter().any(|(p, _)| c.wanted.contains(p)) {
        return Vec::new();
    }
    words(text, caret).into_iter().filter(|r| !correct(&c, &text[r.clone()])).collect()
}

thread_local! {
    static CACHE: std::cell::RefCell<(u64, String, usize, Vec<Range<usize>>)> = Default::default();
}

/// Detect while typing: re-guesses the draft's language unless it only grew or shrank by a
/// few characters since (`isTinyAutodetectPrefixChange`, 20), and checks against it first.
fn follow_draft(text: &str) {
    let tiny = |prev: &str| {
        !prev.is_empty() && !text.is_empty() && prev.len().abs_diff(text.len()) < 20 && (text.starts_with(prev) || prev.starts_with(text))
    };
    {
        let c = CHECKER.read().unwrap();
        if !c.enabled || !c.auto || c.detected_from == text || tiny(&c.detected_from) {
            return;
        }
    }
    let found = if text.trim().is_empty() { None } else { detect_package(text) };
    let mut c = CHECKER.write().unwrap();
    c.detected_from = text.to_string();
    if found.is_some() && found != c.detected {
        c.detected = found;
        let d = found.unwrap_or_default();
        c.wanted = std::iter::once(d).chain(c.preferred.iter().copied().filter(|x| *x != d)).collect();
        c.generation += 1;
        load_missing(&mut c);
    }
}

/// The composer's decorations: a red underline under each misspelling (cached per text).
pub fn spans(text: &str, caret: usize, color: rune::Color) -> Vec<rune::Span> {
    follow_draft(text);
    let generation = CHECKER.read().unwrap().generation;
    CACHE.with_borrow_mut(|(g, t, k, ranges)| {
        if *g != generation || t != text || *k != caret {
            *ranges = misspelled(text, caret);
            (*g, *t, *k) = (generation, text.to_string(), caret);
        }
        ranges.iter().map(|r| rune::Span::new(r.start, r.end).underline(color)).collect()
    })
}

/// The misspelling a context menu is about: the one at the caret, else the last before it,
/// else the first.
// shortcut: Rune gives no text offset for a pointer position, so the caret stands in for it.
pub fn pick(text: &str, caret: usize) -> Option<(Range<usize>, String)> {
    let all = misspelled(text, usize::MAX);
    let r = all.iter().find(|r| r.start <= caret && caret <= r.end).or_else(|| all.iter().rev().find(|r| r.end <= caret)).or(all.first())?;
    Some((r.clone(), text[r.clone()].to_string()))
}

/// Up to five suggestions from the wanted dictionaries.
pub fn suggest(word: &str) -> Vec<String> {
    let dicts: Vec<_> = {
        let c = CHECKER.read().unwrap();
        c.loaded.iter().filter(|(p, _)| c.wanted.contains(p)).map(|(_, d)| d.clone()).collect()
    };
    let mut out = Vec::new();
    for d in dicts {
        let mut v = Vec::new();
        d.suggest(word, &mut v);
        for s in v {
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out.truncate(5);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_skip_links_mentions_code_and_the_caret_word() {
        let t = "helo <@1> https://x.y/abc `codee` don't :smiel: wor";
        let w: Vec<&str> = words(t, t.len()).into_iter().map(|r| &t[r]).collect();
        assert_eq!(w, ["helo", "don't"]);
    }

    #[test]
    fn checks_with_a_dictionary() {
        let aff = "SET UTF-8\nSFX S Y 1\nSFX S 0 s .\n";
        let dic = "2\nhello/S\nworld\n";
        insert("dictionary-en", spellbook::Dictionary::new(aff, dic).unwrap());
        CHECKER.write().unwrap().words.insert("reflux".into());
        let t = "hellos wrold reflux Fluxer x";
        let bad: Vec<&str> = misspelled(t, usize::MAX).into_iter().map(|r| &t[r]).collect();
        assert_eq!(bad, ["wrold"]);
        assert_eq!(pick(t, 0).unwrap().1, "wrold");
        assert!(suggest("wrold").contains(&"world".to_string()));
        assert_eq!(spans(t, usize::MAX, rune::Color::WHITE).len(), 1);
        assert_eq!(for_locale("en-US").unwrap().package, "dictionary-en");
        assert_eq!(for_locale("en-GB").unwrap().package, "dictionary-en-gb");
        assert_eq!(for_locale("pt-BR").unwrap().package, "dictionary-pt");
        assert!(for_locale("ja").is_none());
    }

    #[test]
    fn detects_the_draft_language() {
        assert_eq!(detect_package("Dies ist ein ganz normaler deutscher Satz über das Wetter heute."), Some("dictionary-de"));
        assert_eq!(detect_package("Ceci est une phrase française tout à fait ordinaire sur le temps."), Some("dictionary-fr"));
        assert_eq!(detect_package("오늘 날씨가 정말 좋네요"), Some("dictionary-ko"));
        assert_eq!(detect_package("ok"), None, "too short");
        assert_eq!(package_for("en", &["en-GB".into()]), Some("dictionary-en-gb"));
        assert_eq!(package_for("pt", &["pt-BR".into()]), Some("dictionary-pt"));
    }
}
