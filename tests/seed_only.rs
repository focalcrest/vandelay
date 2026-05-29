/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

mod integration;
mod seeder;

use integration::stalwart::shared as shared_stalwart;

#[test]
#[ignore = "requires Docker"]
fn provision_no_teardown() -> seeder::error::SeedResult<()> {
    let stalwart = shared_stalwart();
    let fx = seeder::provision(stalwart.base_url())?;
    println!(
        "provisioned domain={} (id={}) base_url={}",
        fx.domain, fx.domain_id, fx.base_url
    );
    println!("admin login: {} / {}", fx.admin_login.0, fx.admin_login.1);
    for local in seeder::SYNC_IN {
        if let Some(acc) = fx.account(local) {
            let seeded = acc
                .seeded
                .as_ref()
                .map(|s| {
                    format!(
                        "mailboxes_created={} emails={} contacts={} events={} \
                         address_books={} calendars={} file_nodes={} \
                         sieve_active={:?} identity={}",
                        s.mailboxes_created,
                        s.emails,
                        s.contacts,
                        s.events,
                        s.address_books,
                        s.calendars,
                        s.file_nodes,
                        s.sieve_active,
                        s.identity
                    )
                })
                .unwrap_or_else(|| "(no seed stats)".to_owned());
            println!(
                "{}: email={} password={} account_id={} role={:?} | {seeded}",
                acc.localpart, acc.email, acc.password, acc.account_id, acc.admin_role
            );
        }
    }
    println!("(no teardown; container is torn down when the test binary exits)");
    let _references_teardown: fn(&str) -> seeder::error::SeedResult<()> = seeder::teardown;
    Ok(())
}
