ALTER TABLE `media` ADD `dir` text GENERATED ALWAYS AS (rtrim(rel_path, replace(rel_path, '/', ''))) VIRTUAL NOT NULL;--> statement-breakpoint
CREATE INDEX `media_dir_idx` ON `media` (`root_id`,`dir`);--> statement-breakpoint
ALTER TABLE `tags` ADD `media_count` integer DEFAULT 0 NOT NULL;