ALTER TABLE `media` ADD `rating` integer;--> statement-breakpoint
ALTER TABLE `media` ADD `meta_version` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
CREATE INDEX `media_rating_idx` ON `media` (`rating`,`id`);