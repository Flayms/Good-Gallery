-- What a content search model made of each photo and video (spec 036), in the version of the file it saw: 512 signed
-- bytes (`domain::content::quantize`), empty if the file couldn't be read. Only compared within one `model`.
CREATE TABLE `content_vectors` (
	`media_id` integer NOT NULL,
	`model` text NOT NULL,
	`size` integer NOT NULL,
	`mtime` integer NOT NULL,
	`vector` blob NOT NULL,
	PRIMARY KEY (`media_id`,`model`),
	FOREIGN KEY (`media_id`) REFERENCES `media`(`id`) ON UPDATE no action ON DELETE cascade
);
