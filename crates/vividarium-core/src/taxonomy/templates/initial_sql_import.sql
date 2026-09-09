ATTACH DATABASE 'vividarium_sql_import.db' AS sql_import;
PRAGMA foreign_keys = ON;
BEGIN IMMEDIATE;

-- ============================================================
-- Import rules
--
-- 1. Preserve source taxon IDs and retain only kingdom, order,
--    family, genus, and species, mapped to ranks 1 through 5.
-- 2. Import a retained taxon only when its lineage, including
--    itself, reaches a kingdom (source rank 60).
-- 3. Kingdoms are roots. Other taxa skip unretained ancestors and
--    ancestors of the same or lower rank, then use the nearest
--    remaining ancestor as parent.
-- 4. For rows with the same taxon and name, aggregate them into
--    one row and use MAX(authority_year) as the authority year.
-- 5. Do not import a scientific synonym when it is the same as
--    the taxon's accepted scientific name.
-- ============================================================

-- ============================================================
-- Source database schema
--
-- CREATE TABLE taxa (
--     id INTEGER PRIMARY KEY
--     ,parent INTEGER
--     ,category INTEGER
--     ,rank INTEGER
--     ,scientific_name TEXT
--     ,authority_year TEXT
--     ,geological_range TEXT
--     ,english_name TEXT
-- );
--
-- CREATE TABLE synonyms (
--     parent INTEGER
--     ,category INTEGER
--     ,synonym TEXT
--     ,authority_year TEXT
--     ,PRIMARY KEY (parent, synonym, authority_year)
-- );
--
-- CREATE TABLE chinese (
--     id INTEGER
--     ,is_accepted INTEGER
--     ,chinese_name TEXT
--     ,source TEXT
--     ,PRIMARY KEY (id, chinese_name)
-- );
-- ============================================================

-- ============================================================
-- Target database schema
--
-- CREATE TABLE taxa (
--     taxon_id INTEGER PRIMARY KEY AUTOINCREMENT
--     ,parent_taxon_id INTEGER
--     ,rank INTEGER NOT NULL
--     ,geological_range TEXT
--     ,CHECK (rank IN (1, 2, 3, 4, 5))
--     ,FOREIGN KEY (parent_taxon_id)
--         REFERENCES taxa(taxon_id)
--         ON DELETE RESTRICT
-- );
--
-- CREATE TABLE taxon_names (
--     name_id INTEGER PRIMARY KEY AUTOINCREMENT
--     ,taxon_id INTEGER NOT NULL
--     ,name_type INTEGER NOT NULL
--     ,name TEXT NOT NULL
--     ,normalized_name TEXT
--         GENERATED ALWAYS AS (lower(name)) STORED
--     ,authority_year TEXT
--     ,source TEXT
--     ,CHECK (name_type BETWEEN 1 AND 6)
--     ,CHECK (length(trim(name)) > 0)
--     ,FOREIGN KEY (taxon_id)
--         REFERENCES taxa(taxon_id)
--         ON DELETE CASCADE
-- );
-- CREATE UNIQUE INDEX idx_taxon_names_scientific_family_name
--     ON taxon_names(taxon_id, name) WHERE name_type IN (1, 2);
-- CREATE UNIQUE INDEX idx_taxon_names_chinese_family_name
--     ON taxon_names(taxon_id, name) WHERE name_type IN (3, 4);
-- CREATE UNIQUE INDEX idx_taxon_names_english_family_name
--     ON taxon_names(taxon_id, name) WHERE name_type IN (5, 6);
-- ============================================================

-- Create the SQL Import staging database
CREATE TABLE sql_import.taxa (
    taxon_id INTEGER PRIMARY KEY AUTOINCREMENT
    ,parent_taxon_id INTEGER
    ,rank INTEGER NOT NULL
    ,geological_range TEXT

    ,CHECK (rank IN (1, 2, 3, 4, 5))
    -- rank: 1 = kingdom; 2 = order; 3 = family; 4 = genus; 5 = species

    ,FOREIGN KEY (parent_taxon_id)
        REFERENCES taxa(taxon_id)
        ON DELETE RESTRICT
);

CREATE TABLE sql_import.taxon_names (
    name_id INTEGER PRIMARY KEY AUTOINCREMENT
    ,taxon_id INTEGER NOT NULL
    ,name_type INTEGER NOT NULL
    ,name TEXT NOT NULL
    ,normalized_name TEXT
        GENERATED ALWAYS AS (lower(name)) STORED
    ,authority_year TEXT
    ,source TEXT

    ,CHECK (name_type BETWEEN 1 AND 6)
    -- name_type: 1 = sci_name; 2 = synonym; 3 = zh_name; 4 = zh_alias; 5 = en_name; 6 = en_alias

    ,CHECK (length(trim(name)) > 0)

    ,FOREIGN KEY (taxon_id)
        REFERENCES taxa(taxon_id)
        ON DELETE CASCADE
);

CREATE UNIQUE INDEX sql_import.idx_taxon_names_scientific_family_name
    ON taxon_names(taxon_id, name) WHERE name_type IN (1, 2);
CREATE UNIQUE INDEX sql_import.idx_taxon_names_chinese_family_name
    ON taxon_names(taxon_id, name) WHERE name_type IN (3, 4);
CREATE UNIQUE INDEX sql_import.idx_taxon_names_english_family_name
    ON taxon_names(taxon_id, name) WHERE name_type IN (5, 6);
CREATE INDEX sql_import.idx_taxon_names_taxon_type
    ON taxon_names(taxon_id, name_type);

-- ============================================================

-- Trim ranks
WITH RECURSIVE

-- Map all source taxa once; target_rank is NULL for discarded ranks.
source_taxa AS (
    SELECT
        id AS taxon_id
        ,parent AS source_parent_id
        ,rank AS source_rank
        ,CASE rank
            WHEN 60 THEN 1 -- kingdom
            WHEN 301 THEN 2 -- order
            WHEN 401 THEN 3 -- family
            WHEN 501 THEN 4 -- genus
            WHEN 601 THEN 5 -- species
        END AS target_rank
        ,NULLIF(trim(geological_range), '') AS geological_range
    FROM biolib.taxa
),

-- Retained-rank taxa before lineage validation.
retained AS (
    SELECT
        taxon_id
        ,source_parent_id
        ,source_rank
        ,target_rank
        ,geological_range
    FROM source_taxa
    WHERE target_rank IS NOT NULL
),

-- Walk each retained taxon's complete lineage, including itself.
-- Stop at a kingdom, a missing parent, or a previously visited taxon.
lineage (
    taxon_id -- retained taxon whose lineage is being walked
    ,ancestor_id -- current taxon or ancestor
    ,ancestor_parent_id -- current ancestor's source parent
    ,ancestor_source_rank
    ,depth
    ,path
) AS (
    -- Anchor query: Start from every retained taxon itself.
    SELECT
        taxon_id
        ,taxon_id
        ,source_parent_id
        ,source_rank
        ,0
        ,printf(',%d,', taxon_id)
    FROM retained

    UNION ALL

    -- Recursive query: Continue upward until reaching a kingdom.
    SELECT
        walk.taxon_id
        ,parent.taxon_id
        ,parent.source_parent_id
        ,parent.source_rank
        ,walk.depth + 1
        ,walk.path || parent.taxon_id || ','
    FROM lineage AS walk
    JOIN source_taxa AS parent
        ON walk.ancestor_parent_id = parent.taxon_id
    WHERE walk.ancestor_source_rank <> 60
        -- Prevent malformed cycles from recursing forever.
        AND instr(
            walk.path
            ,printf(',%d,', parent.taxon_id)
        ) = 0
),

-- Keep only taxa whose lineage reaches a kingdom.
valid_taxa AS (
    SELECT
        retained.taxon_id
        ,retained.source_parent_id
        ,retained.target_rank
        ,retained.geological_range
    FROM retained
    WHERE EXISTS (
        SELECT 1
        FROM lineage
        WHERE lineage.taxon_id = retained.taxon_id
            AND lineage.ancestor_source_rank = 60
    )
),

-- Find every retained ancestor whose rank is strictly higher than
-- the child taxon's rank. Invalid retained ancestors are skipped.
parent_candidates AS (
    SELECT
        child.taxon_id
        ,parent.taxon_id AS parent_taxon_id
        ,lineage.depth
        ,ROW_NUMBER() OVER (
            PARTITION BY child.taxon_id
            ORDER BY lineage.depth
        ) AS priority
    FROM valid_taxa AS child
    JOIN lineage
        ON child.taxon_id = lineage.taxon_id
    JOIN valid_taxa AS parent
        ON lineage.ancestor_id = parent.taxon_id
    WHERE lineage.depth > 0
        AND parent.target_rank < child.target_rank
),

-- Select the nearest valid retained ancestor.
nearest_parent AS (
    SELECT
        taxon_id
        ,parent_taxon_id
    FROM parent_candidates
    WHERE priority = 1
)

INSERT INTO sql_import.taxa (
    taxon_id
    ,parent_taxon_id
    ,rank
    ,geological_range
)
SELECT
    valid_taxa.taxon_id
    ,CASE
        WHEN valid_taxa.target_rank = 1 THEN NULL
        ELSE nearest_parent.parent_taxon_id
    END -- kingdom -> no parent
    ,valid_taxa.target_rank
    ,valid_taxa.geological_range
FROM valid_taxa
LEFT JOIN nearest_parent
    ON valid_taxa.taxon_id = nearest_parent.taxon_id
ORDER BY
    valid_taxa.target_rank
    ,valid_taxa.taxon_id;

-- ============================================================

-- Import scientific names
INSERT INTO sql_import.taxon_names (
    taxon_id
    ,name_type
    ,name
    ,authority_year
    ,source
)
SELECT
    source.id
    ,1
    ,source.scientific_name
    ,NULLIF(trim(source.authority_year), '')
    ,'biolib'
FROM biolib.taxa AS source
JOIN sql_import.taxa AS retained
    ON source.id = retained.taxon_id
WHERE source.scientific_name IS NOT NULL
    AND trim(source.scientific_name) <> ''
ORDER BY
    source.id;

-- ============================================================

-- Import synonyms
INSERT INTO sql_import.taxon_names (
    taxon_id
    ,name_type
    ,name
    ,authority_year
    ,source
)
SELECT
    synonym.parent
    ,2
    ,synonym.synonym
    ,synonym.authority_year
    ,'biolib'
FROM (
    SELECT
        parent
        ,synonym
        ,MAX(NULLIF(trim(authority_year), '')) AS authority_year
    FROM biolib.synonyms
    GROUP BY
        parent
        ,synonym
) AS synonym
JOIN biolib.taxa AS source
    ON synonym.parent = source.id
JOIN sql_import.taxa AS retained
    ON synonym.parent = retained.taxon_id
WHERE synonym.synonym IS NOT NULL
    AND trim(synonym.synonym) <> ''
    AND synonym.synonym <> source.scientific_name
ORDER BY
    synonym.parent
    ,synonym.synonym
    ,synonym.authority_year;

-- ============================================================

-- Import Chinese names
WITH valid_chinese AS (
    SELECT
        chinese.id
        ,chinese.is_accepted
        ,chinese.chinese_name
        ,chinese.source
        ,ROW_NUMBER() OVER (
            PARTITION BY chinese.id
            ORDER BY
                chinese.is_accepted DESC
                ,chinese.chinese_name
        ) AS priority -- Prefer the original accepted name; otherwise promote one remaining name.
    FROM biolib.chinese AS chinese
    JOIN sql_import.taxa AS retained
        ON chinese.id = retained.taxon_id
    WHERE chinese.chinese_name IS NOT NULL
        AND trim(chinese.chinese_name) <> ''
)

INSERT INTO sql_import.taxon_names (
    taxon_id
    ,name_type
    ,name
    ,authority_year
    ,source
)
SELECT
    id
    ,CASE
        WHEN priority = 1 THEN 3
        ELSE 4
    END
    ,chinese_name
    ,NULL
    ,NULLIF(trim(source), '')
FROM valid_chinese
ORDER BY
    id
    ,priority;

-- ============================================================

-- Import English names
INSERT INTO sql_import.taxon_names (
    taxon_id
    ,name_type
    ,name
    ,authority_year
    ,source
)
SELECT
    source.id
    ,5
    ,source.english_name
    ,NULL
    ,'biolib'
FROM biolib.taxa AS source
JOIN sql_import.taxa AS retained
    ON source.id = retained.taxon_id
WHERE source.english_name IS NOT NULL
    AND trim(source.english_name) <> ''
ORDER BY
    source.id;

-- ============================================================

COMMIT;
DETACH DATABASE sql_import;
