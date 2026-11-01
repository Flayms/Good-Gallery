-- Named face regions (REQ-004-13), on the upright photo as fractions of its size; the metadata version bump fills them in.
CREATE TABLE `media_faces` (
	`media_id` integer NOT NULL,
	`name` text NOT NULL,
	`x` real NOT NULL,
	`y` real NOT NULL,
	`w` real NOT NULL,
	`h` real NOT NULL,
	FOREIGN KEY (`media_id`) REFERENCES `media`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE INDEX `media_faces_media_idx` ON `media_faces` (`media_id`);
