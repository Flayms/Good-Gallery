-- Edits saved into files (spec 021): the recipe and where the unedited original is.
CREATE TABLE `media_edits` (
	`media_id` integer PRIMARY KEY NOT NULL,
	`recipe` text NOT NULL,
	`source` text NOT NULL,
	FOREIGN KEY (`media_id`) REFERENCES `media`(`id`) ON UPDATE no action ON DELETE cascade
);
