//! Details of single items for the viewer and the info sheet, items outside the search, and faces (spec 032).

use super::*;

impl Gallery {
    /// The details of media item `id` (REQ-009-08), asked for on first use and again after each reload; `None` while
    /// loading.
    pub fn detail(&mut self, id: i64) -> Option<&DetailResult> {
        if self.detail.as_ref().is_none_or(|(asked, _)| *asked != id) {
            self.detail = Some((id, None));
            self.loader.send(Request::Detail { id });
        }
        self.detail.as_ref()?.1.as_ref()
    }

    /// The tile data of an item outside the result, as remembered until the index has answered (REQ-009-18).
    pub fn item_outside(&mut self, id: i64) -> Option<&GalleryItem> {
        if self.outside.as_ref().is_none_or(|(asked, _)| *asked != id) {
            self.outside = Some((id, None));
            self.loader.send(Request::Item { id });
        }
        match &self.outside {
            Some((_, Some(item))) => item.as_ref(),
            _ => self.items.get(&id),
        }
    }

    /// Whether the index said that the item asked for with [`Self::item_outside`] isn't in it.
    pub fn is_outside_gone(&self, id: i64) -> bool {
        matches!(&self.outside, Some((asked, Some(None))) if *asked == id)
    }

    /// The viewer no longer shows an item outside the result.
    pub fn forget_outside(&mut self) {
        self.outside = None;
    }

    /// The details of `id` if they are loaded, without asking for them.
    pub fn cached_detail(&self, id: i64) -> Option<&MediaDetail> {
        match &self.detail {
            Some((asked, Some(Ok(Some(detail))))) if *asked == id => Some(detail),
            _ => None,
        }
    }

    /// Forgets the details asked for last: no info view shows them any more.
    pub fn forget_detail(&mut self) {
        self.detail = None;
    }

    /// Whether a detection model is chosen, so photos may have faces without regions.
    pub fn looks_for_faces(&self) -> bool {
        self.setting(db::faces::DETECTOR_KEY).is_some()
    }

    pub(super) fn face_threshold(&self) -> f32 {
        self.setting(db::faces::THRESHOLD_KEY)
            .and_then(|text| text.parse().ok())
            .unwrap_or(domain::face_models::DEFAULT_THRESHOLD)
    }

    /// The unknown faces on `id`, asked for when it isn't the item asked for last (REQ-032-09).
    pub fn faces_on(&mut self, id: i64) -> &[db::faces::ShownFace] {
        if self.faces.as_ref().is_none_or(|(asked, _)| *asked != id) {
            self.faces = Some((id, None));
            if let Some(detector) = self.setting(db::faces::DETECTOR_KEY) {
                let (recognizer, threshold) = (self.setting(db::faces::RECOGNIZER_KEY), self.face_threshold());
                self.loader.send(Request::Faces {
                    id,
                    detector,
                    recognizer,
                    threshold,
                });
            }
        }
        match &self.faces {
            Some((_, Some(faces))) => faces,
            _ => &[],
        }
    }

    /// Faces changed: they are asked for anew.
    pub fn forget_faces(&mut self) {
        self.faces = None;
        self.face_groups_asked = false;
    }

    /// Takes the faces `gone` says were decided about off the groups shown, until they are formed again (REQ-032-31).
    pub fn drop_from_face_groups(&mut self, gone: impl Fn(&db::faces::GroupFace) -> bool) {
        let Some(groups) = &mut self.face_groups else { return };
        let keep = |group: &mut FaceGroup| group.retain(|(face, _)| !gone(face));
        for (_, group) in &mut groups.suggested {
            keep(group);
        }
        groups.suggested.retain(|(_, group)| !group.is_empty());
        for group in &mut groups.unknown {
            keep(group);
        }
        groups.unknown.retain(|group| !group.is_empty());
        for group in [&mut groups.others, &mut groups.ignored, &mut groups.not_faces] {
            keep(group);
        }
    }

    /// The faces to confirm and the groups of unknown faces, `None` while first loading or without chosen models
    /// (REQ-032-15, REQ-032-24). Once loaded, they stay until they have loaded again (REQ-032-31).
    pub fn face_groups(&mut self) -> Option<&FaceGroups> {
        if !self.face_groups_asked {
            let models = self
                .setting(db::faces::DETECTOR_KEY)
                .zip(self.setting(db::faces::RECOGNIZER_KEY));
            let Some((detector, recognizer)) = models else {
                self.face_groups = None;
                return None;
            };
            self.face_groups_asked = true;
            let threshold = self.face_threshold();
            self.loader.send(Request::FaceGroups {
                detector,
                recognizer,
                threshold,
            });
        }
        self.face_groups.as_ref()
    }
}
