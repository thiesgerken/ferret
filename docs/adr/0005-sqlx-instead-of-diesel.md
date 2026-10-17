# sqlx instead of Diesel

ferret uses sqlx, unlike moneta, which uses Diesel. Most of ferret's important queries are
hand-written SQL with extension operators (`&&&` and `pdb.score` for BM25, `<=>` for vector distance, RRF in CTEs).
Diesel only type-checks its query builder, raw SQL falls back to unchecked `sql_query` (as seen in
moneta). sqlx checks static raw SQL against the real schema at compile time (search, whose clauses vary
per query, is built with `QueryBuilder` and covered by tests instead) and embeds migrations with
checksums that fail loudly when an applied migration was edited.
