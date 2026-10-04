use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::Text;
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

#[derive(Debug, PartialEq, QueryableByName)]
struct Proposal {
    #[diesel(sql_type = Text)]
    id: String,
    #[diesel(sql_type = Text)]
    negotiation_id: String,
    #[diesel(sql_type = Text)]
    properties: String,
}

#[derive(QueryableByName)]
struct QueryPlan {
    #[diesel(sql_type = Text)]
    detail: String,
}

fn database() -> SqliteConnection {
    let mut conn = SqliteConnection::establish(":memory:").unwrap();
    // Match DbExecutor::apply_migration: historical migrations rebuild tables.
    conn.batch_execute("PRAGMA foreign_keys = OFF;").unwrap();
    conn.run_pending_migrations(MIGRATIONS).unwrap();
    conn.batch_execute("PRAGMA foreign_keys = ON;").unwrap();
    conn
}

fn seed(conn: &mut SqliteConnection) {
    conn.batch_execute(
        "INSERT INTO market_negotiation
             (id, subscription_id, offer_id, demand_id, requestor_id, provider_id)
         VALUES ('active', 'subscription', 'offer', 'demand', 'requestor', 'provider'),
                ('expired', 'subscription', 'offer', 'demand', 'requestor', 'provider');
         INSERT INTO market_proposal
             (id, issuer, negotiation_id, properties, constraints, state,
              creation_ts, expiration_ts)
         VALUES ('active-proposal', 'Us', 'active', '{\"value\":1}', '', 'Draft',
                 '2020-01-01', '2035-01-01'),
                ('expired-proposal', 'Us', 'expired', '{\"value\":2}', '', 'Draft',
                 '2020-01-01', '2020-01-02');",
    )
    .unwrap();
}

fn proposals(conn: &mut SqliteConnection) -> Vec<Proposal> {
    diesel::sql_query("SELECT id, negotiation_id, properties FROM market_proposal ORDER BY id")
        .load(conn)
        .unwrap()
}

fn assert_indexed_foreign_key_check(conn: &mut SqliteConnection) {
    let plan =
        diesel::sql_query("EXPLAIN QUERY PLAN DELETE FROM market_negotiation WHERE id = 'expired'")
            .load::<QueryPlan>(conn)
            .unwrap();
    let child_checks: Vec<_> = plan
        .iter()
        .filter(|row| row.detail.contains("market_proposal"))
        .collect();
    assert!(!child_checks.is_empty());
    assert!(child_checks.iter().all(|row| {
        row.detail.contains("SEARCH") && row.detail.contains("market_proposal_negotiation_id_idx")
    }));
}

#[test]
fn negotiation_delete_uses_index_and_preserves_foreign_key_constraints() {
    let mut conn = database();
    seed(&mut conn);
    assert_indexed_foreign_key_check(&mut conn);

    // The performance fix must not allow deletion of a referenced negotiation.
    assert!(
        diesel::sql_query("DELETE FROM market_negotiation WHERE id = 'active'")
            .execute(&mut conn)
            .is_err()
    );
    assert_eq!(proposals(&mut conn).len(), 2);

    assert_eq!(
        diesel::sql_query("DELETE FROM market_proposal WHERE negotiation_id = 'expired'")
            .execute(&mut conn)
            .unwrap(),
        1
    );
    assert_eq!(
        diesel::sql_query("DELETE FROM market_negotiation WHERE id = 'expired'")
            .execute(&mut conn)
            .unwrap(),
        1
    );
    assert_eq!(proposals(&mut conn)[0].id, "active-proposal");
}

#[test]
fn populated_database_upgrade_and_rollback_preserve_proposals() {
    let mut conn = database();
    seed(&mut conn);
    let before = proposals(&mut conn);

    // Reverting the last migration restores the pre-fix schema with live data.
    conn.revert_last_migration(MIGRATIONS).unwrap();
    assert_eq!(proposals(&mut conn), before);
    let old_plan =
        diesel::sql_query("EXPLAIN QUERY PLAN DELETE FROM market_negotiation WHERE id = 'expired'")
            .load::<QueryPlan>(&mut conn)
            .unwrap();
    assert!(old_plan
        .iter()
        .any(|row| { row.detail.contains("SCAN") && row.detail.contains("market_proposal") }));

    let applied = conn.run_pending_migrations(MIGRATIONS).unwrap();
    assert_eq!(applied.len(), 1);
    assert_eq!(proposals(&mut conn), before);
    assert_indexed_foreign_key_check(&mut conn);

    // A restarted daemon has no additional pending migration.
    assert!(conn.run_pending_migrations(MIGRATIONS).unwrap().is_empty());
}
