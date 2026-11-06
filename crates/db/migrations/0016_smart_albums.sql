-- Searches kept under a name (spec 034). `search` is JSON (`domain::smart_albums::search_to_value`).
CREATE TABLE `smart_albums` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`name` text NOT NULL,
	`search` text NOT NULL
);
