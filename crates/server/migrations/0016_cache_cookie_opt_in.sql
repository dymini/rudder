-- Cache rules now skip requests that carry a cookie unless `cache_cookies` is set.
-- Extension rules keep caching for visitors with cookies: their responses are only
-- stored when the Content-Type matches the extension.
UPDATE cache_rules
SET action_spec = action_spec || '{"cache_cookies": true}'::jsonb
WHERE jsonb_typeof(match_spec -> 'extensions') = 'array'
  AND jsonb_array_length(match_spec -> 'extensions') > 0
  AND NOT action_spec ? 'cache_cookies';
