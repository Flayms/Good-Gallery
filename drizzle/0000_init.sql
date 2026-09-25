CREATE TABLE `library_roots` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`path` text NOT NULL,
	`label` text NOT NULL,
	`status` text DEFAULT 'unknown' NOT NULL,
	`last_scan_at` integer
);
--> statement-breakpoint
CREATE UNIQUE INDEX `library_roots_path_unique` ON `library_roots` (`path`);--> statement-breakpoint
CREATE TABLE `media` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`root_id` integer NOT NULL,
	`rel_path` text NOT NULL,
	`file_name` text NOT NULL,
	`kind` text NOT NULL,
	`size` integer NOT NULL,
	`mtime` integer NOT NULL,
	`width` integer,
	`height` integer,
	`duration` real,
	`taken_at` integer,
	`sort_date` integer GENERATED ALWAYS AS (coalesce(taken_at, mtime)) VIRTUAL NOT NULL,
	`thumbhash` text,
	`thumb_status` text DEFAULT 'pending' NOT NULL,
	FOREIGN KEY (`root_id`) REFERENCES `library_roots`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE UNIQUE INDEX `media_root_path_uq` ON `media` (`root_id`,`rel_path`);--> statement-breakpoint
CREATE INDEX `media_date_idx` ON `media` (`sort_date`,`id`);--> statement-breakpoint
CREATE INDEX `media_root_date_idx` ON `media` (`root_id`,`sort_date`,`id`);--> statement-breakpoint
CREATE INDEX `media_name_idx` ON `media` (`file_name`,`id`);--> statement-breakpoint
CREATE TABLE `media_tags` (
	`media_id` integer NOT NULL,
	`tag_id` integer NOT NULL,
	PRIMARY KEY(`media_id`, `tag_id`),
	FOREIGN KEY (`media_id`) REFERENCES `media`(`id`) ON UPDATE no action ON DELETE cascade,
	FOREIGN KEY (`tag_id`) REFERENCES `tags`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE INDEX `media_tags_tag_idx` ON `media_tags` (`tag_id`,`media_id`);--> statement-breakpoint
CREATE TABLE `tags` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`name` text NOT NULL,
	`name_norm` text NOT NULL,
	`parent_id` integer,
	FOREIGN KEY (`parent_id`) REFERENCES `tags`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE UNIQUE INDEX `tags_name_norm_unique` ON `tags` (`name_norm`);--> statement-breakpoint
CREATE INDEX `tags_parent_idx` ON `tags` (`parent_id`);