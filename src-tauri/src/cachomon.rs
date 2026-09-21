// cachomon.com source: the author's Shimeji site (Cachomon and team).
//
// WHAT THIS DOES AND WHAT IT DOES NOT
//
// The app only READS the public list (grid.php): names, franchises,
// thumbnails, authors, complexity and download counts. One request,
// cached for 12 hours.
//
// The app does NOT download files. The site's terms (terms.php) forbid
// downloading its Shimeji "through third-party sites/apps", and the download
// buttons there go through ads or Patreon.
// So a card opens the character page in the browser, and an archive the
// user downloaded themselves is picked up from the Downloads folder
// (see downloads.rs).
//
// Only the SFW list is shown (parameters m=0&a=0, the site's default view).

use crate::library;
use scraper::{ElementRef, Html, Selector};
use serde::{Deserialize, Serialize};

const SITE: &str = "https://cachomon.com";
const GRID: &str = "https://cachomon.com/grid.php?g=1&m=0&a=0&t=0";
const TTL_SECS: u64 = 12 * 3600;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CachoEntry {
    pub id: u32,
    pub name: String,
    /// "Cookie Run", "Pokémon"… (empty if the site gives none).
    pub franchise: String,
    pub thumb: String,
    pub artist: String,
    /// public | beta | exclusive | unavailable
    pub availability: String,
    pub downloads: u32,
    /// "Easy", "Regular", "Difficult"… (without the word Complexity)
    pub complexity: String,
    pub gender: String,
    /// "Shiny Chance", "Sound Effects", "Egg", "Hotspots"…
    pub features: Vec<String>,
    /// The character's page on the site, where the download happens.
    pub url: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CachoIndex {
    pub fetched_at: u64,
    pub entries: Vec<CachoEntry>,
    /// true if the network is unavailable and a saved copy is shown.
    #[serde(default)]
    pub stale: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cache_path() -> std::path::PathBuf {
    library::data_dir().join("cachomon-index.json")
}

fn read_cache() -> Option<CachoIndex> {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn write_cache(idx: &CachoIndex) {
    if std::fs::create_dir_all(library::data_dir()).is_err() {
        return;
    }
    if let Ok(json) = serde_json::to_string(idx) {
        let _ = std::fs::write(cache_path(), json);
    }
}

/// The saved list without touching the network (empty if there is no cache yet).
pub fn cached() -> Vec<CachoEntry> {
    read_cache().map(|c| c.entries).unwrap_or_default()
}

/// The character list: fresh cache → network → stale cache.
pub async fn index(refresh: bool) -> Result<CachoIndex, String> {
    let cached = read_cache();

    if !refresh {
        if let Some(c) = &cached {
            if now().saturating_sub(c.fetched_at) < TTL_SECS && !c.entries.is_empty() {
                return Ok(c.clone());
            }
        }
    }

    match fetch().await {
        Ok(entries) if !entries.is_empty() => {
            let idx = CachoIndex { fetched_at: now(), entries, stale: false };
            write_cache(&idx);
            Ok(idx)
        }
        Ok(_) => Err("cachomon.com returned an empty list; the site layout may have changed".into()),
        Err(e) => match cached {
            Some(mut c) if !c.entries.is_empty() => {
                c.stale = true;
                Ok(c)
            }
            _ => Err(e),
        },
    }
}

async fn fetch() -> Result<Vec<CachoEntry>, String> {
    let body = crate::catalog::client()
        .get(GRID)
        .send()
        .await
        .map_err(|e| format!("could not load cachomon.com: {e}"))?
        .error_for_status()
        .map_err(|e| format!("cachomon.com returned an error: {e}"))?
        .text()
        .await
        .map_err(|e| format!("could not read the cachomon.com response: {e}"))?;

    Ok(parse_grid(&body))
}

fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("valid CSS selector")
}

/// Absolute URL from a relative one ("../thumbnails/x.png", "/images/y.png").
fn absolutize(src: &str) -> String {
    if src.starts_with("http") {
        return src.to_string();
    }
    format!("{SITE}/{}", src.trim_start_matches("../").trim_start_matches('/'))
}

fn titles(el: &ElementRef, css: &Selector, img: &Selector) -> Vec<String> {
    el.select(css)
        .flat_map(|c| c.select(img))
        .filter_map(|i| i.value().attr("title").map(str::to_string))
        .collect()
}

pub fn parse_grid(html: &str) -> Vec<CachoEntry> {
    let doc = Html::parse_document(html);
    let boxes = sel("div.shimejibox");
    let link = sel("a[href*=\"shimeji.php\"]");
    let thumb = sel("img.shimeji");
    let artist = sel("img.artist");
    let top = sel(".shimejiboxtopicons");
    let icons = sel(".shimejiboxicons");
    let img = sel("img");
    let dl = sel(".shimejiboxdownload img");
    let counter = sel(".shimejiboxcounter");
    let text = sel(".shimejitext a");

    let mut out: Vec<CachoEntry> = Vec::new();

    for b in doc.select(&boxes) {
        let Some(id) = b
            .select(&link)
            .filter_map(|a| a.value().attr("href"))
            .find_map(|h| {
                let rest = h.split("id=").nth(1)?;
                rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok()
            })
        else {
            continue;
        };
        if out.iter().any(|e| e.id == id) {
            continue;
        }

        let label = b
            .select(&text)
            .next()
            .map(|a| a.text().collect::<String>())
            .unwrap_or_default();
        let label = label.trim();
        // "???" are beta teasers that are not revealed yet; nothing to show.
        if label.is_empty() || label.starts_with("???") {
            continue;
        }
        let (name, franchise) = match label.split_once(" from ") {
            Some((n, f)) => (n.trim().to_string(), f.trim().to_string()),
            None => (label.to_string(), String::new()),
        };

        let topic = titles(&b, &top, &img);
        // Only general-audience content: skip anything the site does not mark SFW.
        if !topic.iter().any(|t| t == "SFW") {
            continue;
        }
        let complexity = topic
            .iter()
            .find_map(|t| t.strip_suffix(" Complexity"))
            .unwrap_or("")
            .to_string();
        let gender = topic
            .iter()
            .find(|t| matches!(t.as_str(), "Male" | "Female" | "Hermaphrodite" | "Genderless"))
            .cloned()
            .unwrap_or_default();

        let availability = match b.select(&dl).next().and_then(|i| i.value().attr("alt")) {
            Some("Download Now!") => "public",
            Some("Beta") => "beta",
            Some("Patreon Exclusive") => "exclusive",
            _ => "unavailable",
        }
        .to_string();

        out.push(CachoEntry {
            id,
            name,
            franchise,
            thumb: b
                .select(&thumb)
                .next()
                .and_then(|i| i.value().attr("src"))
                .map(absolutize)
                .unwrap_or_default(),
            artist: b
                .select(&artist)
                .next()
                .and_then(|i| i.value().attr("title"))
                .map(|t| t.trim_start_matches("Art by ").to_string())
                .unwrap_or_default(),
            availability,
            downloads: b
                .select(&counter)
                .next()
                .and_then(|c| c.text().collect::<String>().trim().parse().ok())
                .unwrap_or(0),
            complexity,
            gender,
            features: titles(&b, &icons, &img),
            url: format!("{SITE}/shimeji.php?g=1&m=0&a=0&t=0&id={id}"),
        });
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
    <div class="shimejibox">
      <a href="shimeji.php?g=1&amp;m=0&amp;a=0&amp;t=0&amp;id=432" target="_blank"><img src="/thumbnails/biscuitDeer.png" class="shimeji" /></a>
      <a href="grid.php?s=FluffyFoxOfFate"><img src="/images/art_fluffy.png" title="Art by FluffyFoxOfFate" class="artist" /></a>
      <div class="shimejiboxtopicons"><img src="../images/green_icon.png" title="SFW" /><img title="Easy Complexity" /></div>
      <div class="shimejiboxicons"><img title="Hotspots" /></div>
      <div class="shimejiboxfooter"><div class="shimejiboxdownload"><a><img alt="Download Now!" /></a></div>
        <div class="shimejiboxcounter">48</div>
        <div class="shimejitext"><a href="x">Biscuit Deer from Cookie Run</a></div></div>
    </div>
    <div class="shimejibox">
      <a href="shimeji.php?id=435"><img src="/thumbnails/pometaro.png" class="shimeji" /></a>
      <div class="shimejiboxtopicons"><img title="SFW" /></div>
      <div class="shimejiboxfooter"><div class="shimejitext"><a>??? from ???</a></div></div>
    </div>"#;

    #[test]
    fn parses_entry_and_skips_teasers() {
        let v = parse_grid(SAMPLE);
        assert_eq!(v.len(), 1);
        let e = &v[0];
        assert_eq!((e.id, e.name.as_str(), e.franchise.as_str()), (432, "Biscuit Deer", "Cookie Run"));
        assert_eq!(e.thumb, "https://cachomon.com/thumbnails/biscuitDeer.png");
        assert_eq!(e.artist, "FluffyFoxOfFate");
        assert_eq!((e.availability.as_str(), e.downloads, e.complexity.as_str()), ("public", 48, "Easy"));
        assert_eq!(e.features, vec!["Hotspots"]);
    }
}
