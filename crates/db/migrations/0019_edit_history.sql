-- A photo's versions (#247): the recipes it was saved with, as its file stores them (gg:EditHistory).
ALTER TABLE `media_edits` ADD `history` text DEFAULT '' NOT NULL;
