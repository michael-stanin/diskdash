//! Keep rules: which copy to keep. They only produce a suggestion; the user
//! removes the other copies by hand.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeepRule {
    Newest,
    Oldest,
    ShortestPath,
    /// Keep the copy on this drive letter (`D`), else fall back to the shortest path.
    PreferDrive(String),
    /// Keep the copy under this folder, else fall back to the shortest path.
    PreferFolder(String),
}

#[derive(Debug, Clone)]
pub struct Member {
    /// Stable id of the member (the lowest wins a full tie).
    pub id: i64,
    pub path: String,
    /// Last-modified time, unix seconds.
    pub mtime: i64,
    /// Drive letter without colon, upper case.
    pub drive: String,
}

fn under(path: &str, folder: &str) -> bool {
    let (p, f) = (path.to_lowercase(), folder.trim_end_matches('\\').to_lowercase());
    !f.is_empty() && (p == f || p.starts_with(&format!("{f}\\")))
}

fn shortest(ms: &[&Member]) -> Option<i64> {
    ms.iter().min_by_key(|m| (m.path.len(), m.id)).map(|m| m.id)
}

/// Id of the member to keep. Ties break by shortest path, then lowest id.
pub fn choose_keep(rule: &KeepRule, members: &[Member]) -> Option<i64> {
    let all: Vec<&Member> = members.iter().collect();
    match rule {
        KeepRule::ShortestPath => shortest(&all),
        KeepRule::Newest => {
            let best = all.iter().map(|m| m.mtime).max()?;
            shortest(&all.iter().copied().filter(|m| m.mtime == best).collect::<Vec<_>>())
        }
        KeepRule::Oldest => {
            let best = all.iter().map(|m| m.mtime).min()?;
            shortest(&all.iter().copied().filter(|m| m.mtime == best).collect::<Vec<_>>())
        }
        KeepRule::PreferDrive(d) => {
            let d = d.trim_end_matches(':');
            let preferred: Vec<&Member> = all.iter().copied().filter(|m| m.drive.eq_ignore_ascii_case(d)).collect();
            if preferred.is_empty() {
                shortest(&all)
            } else {
                shortest(&preferred)
            }
        }
        KeepRule::PreferFolder(f) => {
            let preferred: Vec<&Member> = all.iter().copied().filter(|m| under(&m.path, f)).collect();
            if preferred.is_empty() {
                shortest(&all)
            } else {
                shortest(&preferred)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(id: i64, path: &str, mtime: i64) -> Member {
        Member { id, path: path.into(), mtime, drive: path[..1].to_uppercase() }
    }

    fn group() -> Vec<Member> {
        vec![m(1, r"C:\Users\a\photo.jpg", 300), m(2, r"D:\Backup\2020\photo.jpg", 100), m(3, r"D:\p.jpg", 300)]
    }

    #[test]
    fn newest_oldest_shortest() {
        let g = group();
        assert_eq!(choose_keep(&KeepRule::Newest, &g), Some(3), "tie on 300 goes to the shorter path");
        assert_eq!(choose_keep(&KeepRule::Oldest, &g), Some(2));
        assert_eq!(choose_keep(&KeepRule::ShortestPath, &g), Some(3));
    }

    #[test]
    fn preferred_drive_and_folder_with_fallback() {
        let g = group();
        assert_eq!(choose_keep(&KeepRule::PreferDrive("D:".into()), &g), Some(3));
        assert_eq!(choose_keep(&KeepRule::PreferDrive("c".into()), &g), Some(1));
        assert_eq!(choose_keep(&KeepRule::PreferDrive("Z".into()), &g), Some(3), "no copy there: shortest path");
        assert_eq!(choose_keep(&KeepRule::PreferFolder(r"d:\backup".into()), &g), Some(2));
        assert_eq!(choose_keep(&KeepRule::PreferFolder(r"D:\Back".into()), &g), Some(3), "folder boundary respected");
    }

    #[test]
    fn full_tie_goes_to_the_lowest_id() {
        let g = vec![m(9, r"C:\a\x.bin", 5), m(4, r"D:\b\x.bin", 5)];
        assert_eq!(choose_keep(&KeepRule::Newest, &g), Some(4));
        assert_eq!(choose_keep(&KeepRule::Newest, &[]), None);
    }
}
