-- Unreadable files (REQ-002-12, REQ-002-15): why the last read of a file's metadata failed, NULL once it succeeds.
ALTER TABLE `media` ADD `meta_error` text;--> statement-breakpoint
CREATE INDEX `media_meta_error_idx` ON `media` (`root_id`,`dir`) WHERE `meta_error` IS NOT NULL;--> statement-breakpoint
-- Earlier versions stored a failed read as empty metadata; without dimensions, it is read again.
UPDATE `media` SET `meta_error` = 'An earlier version read no metadata from it' WHERE `width` IS NULL OR `height` IS NULL;
