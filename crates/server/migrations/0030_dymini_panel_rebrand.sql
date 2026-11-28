-- Rebrand Rudder/Knotree to Dymini Panel: rename database objects and move
-- default storage prefixes. Rows still carrying a stock Rudder-era prefix are
-- moved to the new prefix; custom prefixes are left untouched.

ALTER TABLE rudder_images RENAME TO dymini_panel_images;
ALTER INDEX rudder_images_service_idx RENAME TO dymini_panel_images_service_idx;
ALTER FUNCTION rudder_ensure_daily_partition(text, date) RENAME TO dymini_panel_ensure_daily_partition;

ALTER TABLE backup_jobs ALTER COLUMN path_prefix SET DEFAULT 'dymini-panel';
UPDATE backup_jobs SET path_prefix = 'dymini-panel' WHERE path_prefix = 'rudder';

ALTER TABLE cli_auth_requests ALTER COLUMN client_name SET DEFAULT 'dymini-panel-cli';

ALTER TABLE image_store_settings ALTER COLUMN path_prefix SET DEFAULT 'dymini-panel-images';
UPDATE image_store_settings SET path_prefix = 'dymini-panel-images' WHERE path_prefix = 'rudder-images';

ALTER TABLE panel_settings ALTER COLUMN github_repo SET DEFAULT 'dymini/panel';
UPDATE panel_settings SET github_repo = 'dymini/panel' WHERE github_repo = 'vantanminh/rudder';
