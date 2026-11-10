//! The loader thread: what the gallery asks of the index (layout, tile data, details, sidebar lists, face groups) and applying what comes back.

use super::*;

pub(super) enum Request {
    Faces {
        id: i64,
        detector: String,
        recognizer: Option<String>,
        threshold: f32,
    },
    FaceGroups {
        detector: String,
        recognizer: String,
        threshold: f32,
    },
    Layout {
        generation: u64,
        search: GallerySearch,
    },
    Items {
        generation: u64,
        chunk: usize,
        ids: Vec<i64>,
    },
    Sidebar {
        library: Option<i64>,
    },
    Suggest {
        prefix: String,
        limit: usize,
    },
    Detail {
        id: i64,
    },
    Item {
        id: i64,
    },
    Calendar(CalendarKey),
    /// The located items of a search, for the map (REQ-044-01).
    Located(GallerySearch),
    /// Thumbnail keys of map clusters' newest photos.
    ThumbKeys(Vec<i64>),
}

pub(super) enum Loaded {
    Faces {
        id: i64,
        result: Result<Vec<db::faces::ShownFace>, String>,
    },
    FaceGroups(Result<FaceGroups, String>),
    Layout {
        generation: u64,
        result: Result<GalleryLayout, String>,
    },
    Items {
        generation: u64,
        chunk: usize,
        result: Result<Vec<GalleryItem>, String>,
    },
    Sidebar(Result<SidebarData, String>),
    Suggest {
        prefix: String,
        result: Result<Vec<TagCount>, String>,
    },
    Detail {
        id: i64,
        /// Boxed: the details are much larger than the other results.
        result: Box<DetailResult>,
    },
    Item {
        id: i64,
        result: Result<Option<GalleryItem>, String>,
    },
    Calendar {
        key: CalendarKey,
        result: Result<CalendarData, String>,
    },
    Located {
        search: GallerySearch,
        result: Result<Vec<(i64, f64, f64)>, String>,
    },
    ThumbKeys(Vec<(i64, Option<ThumbKey>)>),
}

/// A media item's details as loaded: `Ok(None)` when it isn't indexed (any more).
pub type DetailResult = Result<Option<MediaDetail>, String>;

/// What the sidebar lists (spec 008), loaded with the gallery.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SidebarData {
    pub people: Vec<TagNode>,
    pub places: Vec<TagNode>,
    /// Media items per rating, highest first, `0` = unrated.
    pub ratings: Vec<(u8, u64)>,
    pub popular: Vec<TagCount>,
    /// Tags outside both categories, most used first: suggestions for the info panel (REQ-026-01).
    pub other_tags: Vec<TagCount>,
    /// The folder tree of the selected library.
    pub folders: Option<(i64, Vec<FolderNode>)>,
    /// Smart albums by name, with their counts (REQ-034-02).
    pub albums: Vec<db::smart_albums::AlbumRow>,
    /// The cameras and lenses in the index, most used first, for the smart album editor (REQ-040-05).
    pub cameras: Vec<(String, u64)>,
    pub lenses: Vec<(String, u64)>,
    /// Media items with a GPS position: the sidebar offers the map when there are any (REQ-044-01).
    pub located: u64,
}

/// Most used tags outside both categories in the sidebar (REQ-008-05).
pub(super) const POPULAR_TAGS: usize = 15;
/// Tags outside both categories suggested when adding one (REQ-026-01).
pub(super) const SUGGESTED_TAGS: usize = 5_000;

/// Existing tags by group, as paths, for adding a tag in the info panel (REQ-026-01).
#[derive(Debug, Clone, Default)]
pub struct TagPaths {
    pub people: Vec<Vec<String>>,
    pub places: Vec<Vec<String>>,
    pub tags: Vec<Vec<String>>,
}

impl TagPaths {
    fn of(sidebar: &SidebarData) -> Self {
        Self {
            people: domain::tagging::paths(domain::tags::PEOPLE_TAG_ROOT, &sidebar.people),
            places: domain::tagging::paths(PLACES_TAG_ROOT, &sidebar.places),
            tags: sidebar.other_tags.iter().map(|tag| vec![tag.name.clone()]).collect(),
        }
    }
}

/// The root a new place goes under (REQ-026-01).
pub const PLACES_TAG_ROOT: &str = "Places";

/// What content words match (REQ-036-05), asked on the loader's thread. The desktop app's runs the chosen model;
/// without one, content words match nothing.
pub trait ContentMatches: Send + Sync {
    fn matches(&self, conn: &Connection, words: &str) -> Result<Vec<i64>, String>;
}

/// The matcher, set once the app has one, shared with the loader's thread.
pub(super) type ContentSlot = Arc<Mutex<Option<Arc<dyn ContentMatches>>>>;

/// `search` as the index can query it: its content words replaced by the items they match (REQ-036-05).
pub(super) fn resolve(
    conn: &Connection,
    search: &GallerySearch,
    content: &ContentSlot,
) -> Result<GallerySearch, String> {
    let matcher = content.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let matches = |words: &str| match &matcher {
        Some(matcher) => matcher.matches(conn, words),
        None => Ok(Vec::new()),
    };
    let mut search = search.clone();
    if let Some(words) = search.content.clone() {
        search = search.with_content_matches(&matches(&words)?);
    }
    // A smart album's _Shows_ condition, on its own (REQ-039-02).
    if let Some(rules) = &search.rules
        && let Some(words) = rules.content()
    {
        let resolved = rules.with_content_matches(&matches(words)?);
        search.rules = Some(Box::new(resolved));
    }
    Ok(search)
}

pub(super) fn load_sidebar(conn: &Connection, library: Option<i64>, content: &ContentSlot) -> db::Result<SidebarData> {
    let folders = match library {
        Some(library) => Some((library, build_folder_tree(&db::tags::library_folders(conn, library)?))),
        None => None,
    };
    Ok(SidebarData {
        people: build_tag_tree(&db::tags::category(conn, TagCategory::People)?),
        places: build_tag_tree(&db::tags::category(conn, TagCategory::Places)?),
        ratings: db::tags::rating_counts(conn)?,
        popular: db::tags::popular(conn, POPULAR_TAGS)?,
        other_tags: db::tags::popular(conn, SUGGESTED_TAGS)?,
        folders,
        // An album whose words can't be matched now counts nothing (REQ-036-07).
        albums: db::smart_albums::rows(conn, &|search| {
            resolve(conn, search, content).unwrap_or_else(|_| search.with_content_matches(&[]))
        })?,
        cameras: db::media::cameras(conn)?,
        lenses: db::media::lenses(conn)?,
        located: db::media::located_count(conn)?,
    })
}

/// Runs one of the gallery's queries.
/// A group of faces, with what is needed to show its thumbnails (REQ-032-15).
pub type FaceGroup = Vec<(db::faces::GroupFace, Option<ThumbKey>)>;

/// What _Unconfirmed faces_ shows: faces to confirm by suggested person (REQ-032-24), then groups of unknown faces
/// (REQ-032-15), the unknown faces in no group (REQ-032-28), and the ignored faces and non-faces (REQ-032-30).
#[derive(Debug, Clone, Default)]
pub struct FaceGroups {
    pub suggested: Vec<(String, FaceGroup)>,
    pub unknown: Vec<FaceGroup>,
    pub others: FaceGroup,
    pub ignored: FaceGroup,
    pub not_faces: FaceGroup,
}

/// Groups shown of each kind; a library with more has far smaller ones further down.
pub(super) const FACE_GROUPS: usize = 40;

pub(super) fn load_face_groups(
    conn: &Connection,
    detector: &str,
    recognizer: &str,
    threshold: f32,
) -> db::Result<FaceGroups> {
    let groups = db::faces::face_groups(conn, detector, recognizer, threshold)?;
    let decided = db::faces::decided_faces(conn, detector)?;
    let with_keys = |faces: Vec<db::faces::GroupFace>| -> db::Result<FaceGroup> {
        faces
            .into_iter()
            .map(|face| {
                let key = db::thumbs::thumb_source(conn, face.media)?.map(|source| source.key);
                Ok((face, key))
            })
            .collect()
    };
    Ok(FaceGroups {
        suggested: groups
            .suggested
            .into_iter()
            .take(FACE_GROUPS)
            .map(|(name, faces)| Ok((name, with_keys(faces)?)))
            .collect::<db::Result<_>>()?,
        unknown: groups
            .unknown
            .into_iter()
            .take(FACE_GROUPS)
            .map(with_keys)
            .collect::<db::Result<_>>()?,
        others: with_keys(groups.others)?,
        ignored: with_keys(decided.ignored)?,
        not_faces: with_keys(decided.not_faces)?,
    })
}

pub(super) fn run(conn: &Connection, request: Request, content: &ContentSlot) -> Loaded {
    match request {
        Request::Faces {
            id,
            detector,
            recognizer,
            threshold,
        } => Loaded::Faces {
            id,
            result: db::faces::shown_faces(conn, id, &detector, recognizer.as_deref(), threshold)
                .map_err(|e| e.to_string()),
        },
        Request::FaceGroups {
            detector,
            recognizer,
            threshold,
        } => Loaded::FaceGroups(load_face_groups(conn, &detector, &recognizer, threshold).map_err(|e| e.to_string())),
        Request::Layout { generation, search } => Loaded::Layout {
            generation,
            result: resolve(conn, &search, content)
                .and_then(|search| db::gallery::layout(conn, &search).map_err(|e| e.to_string())),
        },
        Request::Items { generation, chunk, ids } => Loaded::Items {
            generation,
            chunk,
            result: db::gallery::items(conn, &ids).map_err(|e| e.to_string()),
        },
        Request::Sidebar { library } => {
            Loaded::Sidebar(load_sidebar(conn, library, content).map_err(|e| e.to_string()))
        }
        Request::Suggest { prefix, limit } => Loaded::Suggest {
            result: db::tags::autocomplete(conn, &prefix, limit).map_err(|e| e.to_string()),
            prefix,
        },
        Request::Calendar(key) => Loaded::Calendar {
            result: load_calendar(conn, &key, content),
            key,
        },
        Request::Located(search) => Loaded::Located {
            result: resolve(conn, &search, content)
                .and_then(|resolved| db::gallery::located_items(conn, &resolved).map_err(|e| e.to_string())),
            search,
        },
        Request::ThumbKeys(ids) => Loaded::ThumbKeys(
            ids.into_iter()
                .map(|id| {
                    let key = db::thumbs::thumb_source(conn, id)
                        .ok()
                        .flatten()
                        .map(|source| source.key);
                    (id, key)
                })
                .collect(),
        ),
        Request::Detail { id } => Loaded::Detail {
            id,
            result: Box::new(db::media::detail(conn, id).map_err(|e| e.to_string())),
        },
        Request::Item { id } => Loaded::Item {
            id,
            result: db::gallery::items(conn, &[id])
                .map(|mut items| items.pop())
                .map_err(|e| e.to_string()),
        },
    }
}

/// Runs the gallery's queries on its own thread, so a 500k-row layout never stalls a frame. The browser has no threads:
/// there they run when they are sent, which the libraries in scope allow (spec 019, Assumptions).
pub(super) struct Loader {
    #[cfg(not(target_arch = "wasm32"))]
    pub(super) requests: Sender<Request>,
    #[cfg(target_arch = "wasm32")]
    pub(super) conn: Arc<Mutex<Connection>>,
    #[cfg(target_arch = "wasm32")]
    pub(super) outbox: Sender<Loaded>,
    /// What content words match, once the app has a matcher.
    pub(super) content: ContentSlot,
    pub(super) results: Receiver<Loaded>,
    /// Requests sent and not yet applied.
    pub(super) pending: usize,
}

impl Loader {
    #[cfg(target_arch = "wasm32")]
    pub(super) fn start(conn: Arc<Mutex<Connection>>, _ctx: Option<egui::Context>) -> Self {
        let (outbox, results) = channel();
        Self {
            conn,
            outbox,
            content: ContentSlot::default(),
            results,
            pending: 0,
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(super) fn send(&mut self, request: Request) {
        let loaded = run(
            &self.conn.lock().unwrap_or_else(|p| p.into_inner()),
            request,
            &self.content,
        );
        if self.outbox.send(loaded).is_ok() {
            self.pending += 1;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn start(conn: Arc<Mutex<Connection>>, ctx: Option<egui::Context>) -> Self {
        let (requests, inbox) = channel::<Request>();
        let (outbox, results) = channel();
        let content = ContentSlot::default();
        let shared = content.clone();
        std::thread::Builder::new()
            .name("gallery".into())
            .spawn(move || {
                while let Ok(request) = inbox.recv() {
                    let conn = conn.lock().unwrap_or_else(|p| p.into_inner());
                    let loaded = run(&conn, request, &shared);
                    drop(conn);
                    if outbox.send(loaded).is_err() {
                        return;
                    }
                    if let Some(ctx) = &ctx {
                        ctx.request_repaint();
                    }
                }
            })
            .expect("can't start the gallery thread");
        Self {
            requests,
            content,
            results,
            pending: 0,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(super) fn send(&mut self, request: Request) {
        if self.requests.send(request).is_ok() {
            self.pending += 1;
        }
    }
}

impl Gallery {
    pub(super) fn apply(&mut self, loaded: Loaded) {
        self.loader.pending -= 1;
        match loaded {
            Loaded::Layout { generation, result } => {
                if generation != self.generation {
                    return;
                }
                match result {
                    Ok(data) => {
                        // Items that left the index leave the selection (REQ-025-05).
                        self.selection.retain(&data.ids);
                        self.data = Some(Arc::new(data));
                        self.data_version += 1;
                        self.error = None;
                        // Positions moved: chunks are loaded again (remembered items stay shown meanwhile), and
                        // thumbnails found missing may have been rendered since.
                        self.chunks.clear();
                        if let Some(thumbnails) = &mut self.thumbnails {
                            thumbnails.streamer.retry_all();
                        }
                        if let Some((id, _)) = self.detail {
                            self.loader.send(Request::Detail { id });
                        }
                        if let Some((id, _)) = self.outside {
                            self.loader.send(Request::Item { id });
                        }
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            Loaded::Sidebar(result) => match result {
                Ok(data) => {
                    self.tag_paths = Arc::new(TagPaths::of(&data));
                    self.sidebar = Some(data);
                }
                Err(error) => tracing::error!("can't load the sidebar: {error}"),
            },
            Loaded::Item { id, result } => match result {
                Ok(item) => {
                    if let Some((asked, loaded)) = &mut self.outside
                        && *asked == id
                    {
                        *loaded = Some(item);
                    }
                }
                Err(error) => tracing::error!("can't load an item: {error}"),
            },
            Loaded::Faces { id, result } => {
                if let Some((asked, loaded)) = &mut self.faces
                    && *asked == id
                {
                    *loaded = Some(result.unwrap_or_else(|error| {
                        tracing::error!("can't load the faces of media item {id}: {error}");
                        Vec::new()
                    }));
                }
            }
            Loaded::Located { search, result } => match result {
                Ok(points) if self.map_asked.as_ref() == Some(&search) => self.map_points = Some(Arc::new(points)),
                Ok(_) => {}
                Err(error) => tracing::error!("can't load the map's photos: {error}"),
            },
            Loaded::ThumbKeys(keys) => self.map_keys.extend(keys),
            Loaded::Calendar { key, result } => match result {
                Ok(data) => self.calendar_data = Some((key, data)),
                Err(error) => tracing::error!("can't load the calendar's days: {error}"),
            },
            Loaded::FaceGroups(result) => {
                self.face_groups = Some(result.unwrap_or_else(|error| {
                    tracing::error!("can't load the groups of faces: {error}");
                    Default::default()
                }));
            }
            Loaded::Detail { id, result } => {
                if let Some((asked, loaded)) = &mut self.detail
                    && *asked == id
                {
                    if let Err(error) = result.as_ref() {
                        tracing::error!("can't load the details of media item {id}: {error}");
                    }
                    *loaded = Some(*result);
                }
            }
            Loaded::Suggest { prefix, result } => match result {
                Ok(tags) => self.suggestions = Some((prefix, tags)),
                Err(error) => tracing::error!("can't load tag suggestions: {error}"),
            },
            Loaded::Items {
                generation,
                chunk,
                result,
            } => match result {
                Ok(items) => {
                    if self.items.len() > MAX_REMEMBERED {
                        self.items.clear();
                        self.placeholders.clear();
                        // Forgotten chunks are asked for again when in view.
                        self.chunks.clear();
                    }
                    for item in items {
                        // A rewritten file (an edit saved over it): its thumbnail texture and placeholder are stale.
                        if let Some(old) = self.items.get(&item.id)
                            && (old.size, old.mtime, &old.thumbhash) != (item.size, item.mtime, &item.thumbhash)
                        {
                            self.placeholders.remove(&item.id);
                            if let Some(thumbnails) = &mut self.thumbnails {
                                thumbnails.streamer.forget(item.id);
                            }
                        }
                        self.items.insert(item.id, item);
                    }
                }
                Err(error) => {
                    tracing::error!("can't load gallery items: {error}");
                    // Asked again when the range is next in view.
                    if generation == self.data_version {
                        self.chunks.remove(&chunk);
                    }
                }
            },
        }
    }
}
