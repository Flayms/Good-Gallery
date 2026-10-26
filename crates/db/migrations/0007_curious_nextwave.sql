DROP INDEX `media_name_idx`;--> statement-breakpoint
ALTER TABLE `media` ADD `file_name_lower` text GENERATED ALWAYS AS (lower(file_name)) VIRTUAL NOT NULL;--> statement-breakpoint
CREATE INDEX `media_mtime_idx` ON `media` (`mtime`,`id`);--> statement-breakpoint
CREATE INDEX `media_size_idx` ON `media` (`size`,`id`);--> statement-breakpoint
CREATE INDEX `media_name_idx` ON `media` (`file_name_lower`,`id`);