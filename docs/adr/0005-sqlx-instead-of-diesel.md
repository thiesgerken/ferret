# sqlx instead of Diesel

ferret uses sqlx, unlike moneta, which uses Diesel. Most of ferret's important queries are
hand-written SQL with extension operators (`@@@` for BM25, `<=>` for vector distance, RRF in CTEs).
Diesel only type-checks its query builder, raw SQL falls back to unchecked `sql_query` (as seen in
moneta). sqlx checks raw SQL against the real schema at compile time and embeds migrations with
checksums that fail loudly when an applied migration was edited.
