-- Kind filters (videos of a year, REQ-007-14) range-scan this index in date order instead of checking every row of the range.
CREATE INDEX `media_kind_date_idx` ON `media` (`kind`,`sort_date`,`id`);
