-- Faces found by a detection model (spec 032), on the upright photo as fractions of its size. Names aren't here: a face overlapping a named region in `media_faces` is that person.
CREATE TABLE `detected_faces` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`media_id` integer NOT NULL,
	`detector` text NOT NULL,
	`x` real NOT NULL,
	`y` real NOT NULL,
	`w` real NOT NULL,
	`h` real NOT NULL,
	`landmarks` blob NOT NULL,
	`score` real NOT NULL,
	`state` text DEFAULT 'open' NOT NULL,
	FOREIGN KEY (`media_id`) REFERENCES `media`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
CREATE INDEX `detected_faces_media_idx` ON `detected_faces` (`media_id`,`detector`);
--> statement-breakpoint
-- What a recognition model made of a face; only compared within one `recognizer` (REQ-032-07).
CREATE TABLE `face_embeddings` (
	`face_id` integer NOT NULL,
	`recognizer` text NOT NULL,
	`vector` blob NOT NULL,
	PRIMARY KEY (`face_id`,`recognizer`),
	FOREIGN KEY (`face_id`) REFERENCES `detected_faces`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
-- People the user said a face isn't (REQ-032-10).
CREATE TABLE `face_rejections` (
	`face_id` integer NOT NULL,
	`name` text NOT NULL,
	PRIMARY KEY (`face_id`,`name`),
	FOREIGN KEY (`face_id`) REFERENCES `detected_faces`(`id`) ON UPDATE no action ON DELETE cascade
);
--> statement-breakpoint
-- Photos a detection model has looked at, in the version of the file it saw (REQ-032-05).
CREATE TABLE `faces_scanned` (
	`media_id` integer NOT NULL,
	`detector` text NOT NULL,
	`size` integer NOT NULL,
	`mtime` integer NOT NULL,
	PRIMARY KEY (`media_id`,`detector`),
	FOREIGN KEY (`media_id`) REFERENCES `media`(`id`) ON UPDATE no action ON DELETE cascade
);
