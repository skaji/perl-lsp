-- Reports over bench/measurements.duckdb.
--   duckdb bench/measurements.duckdb < bench/report.sql
--
-- Every aggregate reports N and SPREAD alongside the value. A mean with n=1
-- is a sample wearing a mean's clothes, and that is precisely how a phantom
-- +400ms regression survived a day here. If n<3, the number is provisional
-- and the report says so rather than letting the reader assume otherwise.

.mode box

SELECT '── headline: wall + peak RSS, per corpus/phase ──' AS section;
SELECT corpus, phase, name,
       count(*)                                   AS n,
       round(median(value), 1)                    AS median,
       round(min(value), 1)                       AS min,
       round(max(value), 1)                       AS max,
       round(max(value) - min(value), 1)          AS spread,
       CASE WHEN count(*) < 3 THEN 'PROVISIONAL (n<3)' ELSE '' END AS caveat
FROM measurements
WHERE kind IN ('timing', 'rss')
GROUP BY corpus, phase, name
ORDER BY corpus, phase, name;

SELECT '── slowest files: what a whole-corpus total hides ──' AS section;
SELECT corpus, name AS file,
       count(*) AS n, round(median(value), 1) AS median_ms
FROM measurements
WHERE kind = 'file_build'
GROUP BY corpus, name
ORDER BY median_ms DESC
LIMIT 15;

SELECT '── build-time distribution: a tail is invisible in a mean ──' AS section;
SELECT corpus,
       count(*)                                        AS files,
       round(sum(value), 1)                            AS total_ms,
       round(median(value), 1)                         AS p50,
       round(quantile_cont(value, 0.99), 1)            AS p99,
       round(max(value), 1)                            AS max,
       round(max(value) / nullif(median(value), 0), 1) AS max_over_p50
FROM measurements
WHERE kind = 'file_build'
GROUP BY corpus
ORDER BY max_over_p50 DESC;

SELECT '── counters, cold vs warm (top movers) ──' AS section;
SELECT corpus, name,
       round(median(CASE WHEN phase = 'cold' THEN value END)) AS cold,
       round(median(CASE WHEN phase = 'warm' THEN value END)) AS warm
FROM measurements
WHERE kind = 'counter'
GROUP BY corpus, name
HAVING cold IS DISTINCT FROM warm
ORDER BY abs(coalesce(cold, 0) - coalesce(warm, 0)) DESC
LIMIT 15;

SELECT '── regression: same corpus across SHAs ──' AS section;
-- The comparison that matters. Flags only when the delta clears the observed
-- spread of BOTH sides: a move smaller than the noise you measured is not a
-- finding, it is the noise. Silent when there is only one SHA.
WITH per_sha AS (
  SELECT r.sha, r.ts, m.corpus, m.phase, m.name,
         count(*) AS n, median(m.value) AS med,
         max(m.value) - min(m.value) AS spread
  FROM measurements m JOIN runs r USING (run_id)
  WHERE m.kind IN ('timing', 'rss')
  GROUP BY r.sha, r.ts, m.corpus, m.phase, m.name
),
paired AS (
  SELECT corpus, phase, name, sha, ts, med, spread, n,
         lag(med)    OVER w AS prev_med,
         lag(spread) OVER w AS prev_spread,
         lag(sha)    OVER w AS prev_sha
  FROM per_sha
  WINDOW w AS (PARTITION BY corpus, phase, name ORDER BY ts)
)
SELECT corpus, phase, name, prev_sha, sha,
       round(prev_med, 1) AS before, round(med, 1) AS after,
       round(med - prev_med, 1) AS delta,
       round(greatest(spread, prev_spread), 1) AS noise
FROM paired
WHERE prev_med IS NOT NULL
  AND abs(med - prev_med) > greatest(spread, prev_spread, 1)
ORDER BY abs(med - prev_med) / nullif(prev_med, 0) DESC;
