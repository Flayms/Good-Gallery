CREATE TABLE `folders` (
	`root_id` integer NOT NULL,
	`rel_dir` text NOT NULL,
	`mtime` integer NOT NULL,
	PRIMARY KEY(`root_id`, `rel_dir`),
	FOREIGN KEY (`root_id`) REFERENCES `library_roots`(`id`) ON UPDATE no action ON DELETE cascade
);
