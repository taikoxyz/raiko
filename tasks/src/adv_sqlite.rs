// Raiko
// Copyright (c) 2024 Taiko Labs
// Licensed and distributed under either of
//   * MIT license (license terms in the root directory or at http://opensource.org/licenses/MIT).
//   * Apache v2 license (license terms in the root directory or at http://www.apache.org/licenses/LICENSE-2.0).
// at your option. This file may not be copied, modified, or distributed except according to those terms.

//! # Raiko Task Manager
//!
//! At the moment (Apr '24) proving requires a significant amount of time
//! and maintaining a connection with a potentially external party.
//!
//! By design Raiko is stateless, it prepares inputs and forward to the various proof systems.
//! However some proving backend like Risc0's Bonsai are also stateless,
//! and only accepts proofs and return result.
//! Hence to handle crashes, networking losses and restarts, we need to persist
//! the status of proof requests, task submitted, proof received, proof forwarded.
//!
//! In the diagram:
//!              _____________          ______________             _______________
//! Taiko L2 -> | Taiko-geth | ======> | Raiko-host  | =========> | Raiko-guests |
//!             | Taiko-reth |         |             |            |     Risc0    |
//!             |____________|         |_____________|            |     SGX      |
//!                                                               |     SP1      |
//!                                                               |______________|
//!                                                                _____________________________
//!                                                    =========> |        Prover Networks     |
//!                                                               |        Risc0's Bonsai      |
//!                                                               |  Succinct's Prover Network |
//!                                                               |____________________________|
//!                                                               _________________________
//!                                                    =========> |       Raiko-dist      |
//!                                                               |    Distributed Risc0  |
//!                                                               |    Distributed SP1    |
//!                                                               |_______________________|
//!
//! We would position Raiko task manager either before Raiko-host or after Raiko-host.
//!
//! ## Implementation
//!
//! The task manager is a set of tables and KV-stores.
//! - Keys for table joins are prefixed with id
//! - KV-stores for (almost) immutable data
//! - KV-store for large inputs and indistinguishable from random proofs
//! - Tables for tasks and their metadata.
//!
//!  __________________________
//! | metadata                |
//! |_________________________| A simple KV-store with the DB version for migration/upgrade detection.
//! | Key             | Value | Future version may add new fields, without breaking older versions.
//! |_________________|_______|
//! | task_db_version | 0     |
//! |_________________|_______|
//!
//! ________________________
//! | Proof systems        |
//! |______________________| A map: ID -> proof systems
//! | id_proofsys | Desc   |
//! |_____________|________|
//! | 0           | Risc0  | (0 for Risc0 and 1 for SP1 is intentional)
//! | 1           | SP1    |
//! | 2           | SGX    |
//! |_____________|________|
//!
//!  _________________________________________________
//! | Task Status code                               |
//! |________________________________________________|
//! | id_status   | Desc                             |
//! |_____________|__________________________________|
//! |     0       | Success                          |
//! |  1000       | Registered                       |
//! |  2000       | Work-in-progress                 |
//! |             |                                  |
//! | -1000       | Proof failure (prover - generic) |
//! | -1100       | Proof failure (OOM)              |
//! |             |                                  |
//! | -2000       | Network failure                  |
//! |             |                                  |
//! | -3000       | Cancelled                        |
//! | -3100       | Cancelled (never started)        |
//! | -3200       | Cancelled (aborted)              |
//! | -3210       | Cancellation in progress         | (Yes -3210 is intentional ;))
//! |             |                                  |
//! | -4000       | Invalid or unsupported block     |
//! |             |                                  |
//! | -9999       | Unspecified failure reason       |
//! |_____________|__________________________________|
//!
//! Rationale:
//! - Convention, failures use negative status code.
//! - We leave space for new status codes
//! - -X000 status code are for generic failures segregated by failures:
//!   on the networking side, the prover side or trying to prove an invalid block.
//!
//!   A catchall -9999 error code is provided if a failure is not due to
//!   either the network, the prover or the requester invalid block.
//!   They should not exist in the DB and a proper analysis
//!   and eventually status code should be assigned.
//!
//!  ________________________________________________________________________________________________
//! | Tasks metadata                                                                                 |
//! |________________________________________________________________________________________________|
//! | id_task | chain_id | block_number | blockhash | parent_hash | state_root | # of txs | gas_used |
//! |_________|__________|______________|___________|_____________|____________|__________|__________|
//!  ____________________________________
//! | Task queue                        |
//! |___________________________________|
//! | id_task | blockhash | id_proofsys |
//! |_________|___________|_____________|
//!  ______________________________________
//! | Task payloads                       |
//! |_____________________________________|
//! | id_task | inputs (serialized)       |
//! |_________|___________________________|
//!  _____________________________________
//! | Task requests                      |
//! |____________________________________|
//! | id_task | id_submitter | timestamp |
//! |_________|______________|___________|
//!  ___________________________________________________________________________________
//! | Task progress trail                                                              |
//! |__________________________________________________________________________________|
//! | id_task | third_party            | id_status               | timestamp           |
//! |_________|________________________|_________________________|_____________________|
//! |  101    | 'Based Proposer"       |  1000 (Registered)      | 2024-01-01 00:00:01 |
//! |  101    | 'A Prover Network'     |  2000 (WIP)             | 2024-01-01 00:00:01 |
//! |  101    | 'A Prover Network'     | -2000 (Network failure) | 2024-01-01 00:02:00 |
//! |  101    | 'Proof in the Pudding' |  2000 (WIP)             | 2024-01-01 00:02:30 |
//!·|  101    | 'Proof in the Pudding' |     0 (Success)         | 2024-01-01 01:02:30 |
//!
//! Rationale:
//! - payloads are very large and warrant a dedicated table, with pruning
//! - metadata is useful to audit block building and prover efficiency
//! - Due to failures and retries, we may submit the same task to multiple fulfillers
//!   or retry with the same fulfiller so we keep an audit trail of events.
//!
//! ____________________________
//! | Proof cache               | A map: ID -> proof
//! |___________________________|
//! | id_task  | proof_value    |
//! |__________|________________|  A Groth16 proof is 2G₁+1G₂ elements
//! | 0        | 0xabcd...6789  |  On BN254: 2*(2*32)+1*(2*2*32) = 256 bytes
//! | 1        | 0x1234...cdef  |
//! | ...      | ...            |  A SGX proof is ...
//! |__________|________________|  A Stark proof (not wrapped in Groth16) would be several kilobytes
//!
//! Do we need pruning?
//!   There are 60s * 60min * 24h * 30j = 2592000s in a month
//!   dividing by 12, that's 216000 Ethereum slots.
//!   Assuming 1kB of proofs per block (Stark-to-Groth16 Risc0 & SP1 + SGX, SGX size to be verified)
//!   That's only 216MB per month.

// Imports
// ----------------------------------------------------------------
use std::{
    fs::File,
    path::Path,
    sync::{Arc, Once},
};

use chrono::{DateTime, Utc};
use raiko_lib::{
    primitives::B256,
    prover::{IdStore, IdWrite, ProofKey, ProverError, ProverResult},
};
use rusqlite::{
    named_params, {Connection, OpenFlags},
};
use tokio::{runtime::Builder, sync::Mutex};

use crate::{
    TaskDescriptor, TaskManager, TaskManagerError, TaskManagerOpts, TaskManagerResult,
    TaskProvingStatus, TaskProvingStatusRecords, TaskReport, TaskStatus,
};

// Types
// ----------------------------------------------------------------

#[derive(Debug)]
pub struct TaskDb {
    conn: Connection,
}

pub struct SqliteTaskManager {
    arc_task_db: Arc<Mutex<TaskDb>>,
}

// Implementation
// ----------------------------------------------------------------

impl TaskDb {
    fn open(path: &Path) -> TaskManagerResult<Connection> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        Ok(conn)
    }

    fn create(path: &Path) -> TaskManagerResult<Connection> {
        let _file = File::options()
            .write(true)
            .read(true)
            .create_new(true)
            .open(path)?;

        let conn = Self::open(path)?;
        Self::create_tables(&conn)?;
        Self::create_views(&conn)?;

        Ok(conn)
    }

    /// Open an existing TaskDb database at "path"
    /// If a database does not exist at the path, one is created.
    pub fn open_or_create(path: &Path) -> TaskManagerResult<Self> {
        let conn = if path.exists() {
            Self::open(path)
        } else {
            Self::create(path)
        }?;
        Ok(Self { conn })
    }

    // SQL
    // ----------------------------------------------------------------

    fn create_tables(conn: &Connection) -> TaskManagerResult<()> {
        // Change the task_db_version if backward compatibility is broken
        // and introduce a migration on DB opening ... if conserving history is important.
        conn.execute_batch(
            r#"
            -- Key value store
            -----------------------------------------------
            CREATE TABLE store(
              chain_id INTEGER NOT NULL,
              blockhash BLOB NOT NULL,
              proofsys_id INTEGER NOT NULL,
              id TEXT NOT NULL,
              FOREIGN KEY(proofsys_id) REFERENCES proofsys(id),
              UNIQUE (chain_id, blockhash, proofsys_id)
            );

            -- Metadata and mappings
            -----------------------------------------------
            CREATE TABLE metadata(
              key BLOB UNIQUE NOT NULL PRIMARY KEY,
              value BLOB
            );
            
            INSERT INTO
              metadata(key, value)
            VALUES
              ('task_db_version', 0);
            
            CREATE TABLE proofsys(
              id INTEGER UNIQUE NOT NULL PRIMARY KEY,
              desc TEXT NOT NULL
            );
            
            INSERT INTO
              proofsys(id, desc)
            VALUES
              (0, 'Native'),
              (1, 'SP1'),
              (2, 'SGX'),
              (3, 'Risc0');
            
            CREATE TABLE status_codes(
              id INTEGER UNIQUE NOT NULL PRIMARY KEY,
              desc TEXT NOT NULL
            );
            
            INSERT INTO
              status_codes(id, desc)
            VALUES
              (0, 'Success'),
              (1000, 'Registered'),
              (2000, 'Work-in-progress'),
              (-1000, 'Proof failure (generic)'),
              (-1100, 'Proof failure (Out-Of-Memory)'),
              (-2000, 'Network failure'),
              (-3000, 'Cancelled'),
              (-3100, 'Cancelled (never started)'),
              (-3200, 'Cancelled (aborted)'),
              (-3210, 'Cancellation in progress'),
              (-4000, 'Invalid or unsupported block'),
              (-9999, 'Unspecified failure reason');
            
            -- Data
            -----------------------------------------------
            -- Notes:
            --   1. a blockhash may appear as many times as there are prover backends.
            --   2. For query speed over (chain_id, blockhash)
            --      there is no need to create an index as the UNIQUE constraint
            --      has an implied index, see:
            --      - https://sqlite.org/lang_createtable.html#uniqueconst
            --      - https://www.sqlite.org/fileformat2.html#representation_of_sql_indices
            CREATE TABLE tasks(
              id INTEGER UNIQUE NOT NULL PRIMARY KEY,
              chain_id INTEGER NOT NULL,
              blockhash BLOB NOT NULL,
              proofsys_id INTEGER NOT NULL,
              prover TEXT NOT NULL,
              FOREIGN KEY(proofsys_id) REFERENCES proofsys(id),
              UNIQUE (chain_id, blockhash, proofsys_id)
            );
            
            -- Proofs might also be large, so we isolate them in a dedicated table
            CREATE TABLE task_proofs(
              task_id INTEGER UNIQUE NOT NULL PRIMARY KEY,
              proof TEXT,
              FOREIGN KEY(task_id) REFERENCES tasks(id)
            );
            
            CREATE TABLE task_status(
              task_id INTEGER NOT NULL,
              status_id INTEGER NOT NULL,
              timestamp TIMESTAMP DEFAULT (STRFTIME('%Y-%m-%d %H:%M:%f', 'NOW')) NOT NULL,
              FOREIGN KEY(task_id) REFERENCES tasks(id),
              FOREIGN KEY(status_id) REFERENCES status_codes(id),
              UNIQUE (task_id, timestamp)
            );
            "#,
        )?;

        Ok(())
    }

    fn create_views(conn: &Connection) -> TaskManagerResult<()> {
        // By convention, views will use an action verb as name.
        conn.execute_batch(
            r#"
            CREATE VIEW enqueue_task AS
            SELECT
              t.id,
              t.chain_id,
              t.blockhash,
              t.proofsys_id,
              t.prover
            FROM
              tasks t
              LEFT JOIN task_status ts on ts.task_id = t.id;
            
            CREATE VIEW update_task_progress AS
            SELECT
              t.id,
              t.chain_id,
              t.blockhash,
              t.proofsys_id,
              t.prover,
              ts.status_id,
              tpf.proof
            FROM
              tasks t
              LEFT JOIN task_status ts on ts.task_id = t.id
              LEFT JOIN task_proofs tpf on tpf.task_id = t.id;
            "#,
        )?;

        Ok(())
    }

    /// Set a tracer to debug SQL execution
    /// for example:
    ///   db.set_tracer(Some(|stmt| println!("sqlite:\n-------\n{}\n=======", stmt)));
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn set_tracer(&mut self, trace_fn: Option<fn(_: &str)>) {
        self.conn.trace(trace_fn);
    }

    pub fn manage(&self) -> TaskManagerResult<()> {
        // To update all the tables with the task_id assigned by Sqlite
        // we require row IDs for the tasks table
        // and we use last_insert_rowid() which is not reentrant and need a transaction lock
        // and store them in a temporary table, configured to be in-memory.
        //
        // Alternative approaches considered:
        // 1. Sqlite does not support variables (because it's embedded and significantly less overhead than other SQL "Client-Server" DBs).
        // 2. using AUTOINCREMENT and/or the sqlite_sequence table
        //		- sqlite recommends not using AUTOINCREMENT for performance
        //        https://www.sqlite.org/autoinc.html
        // 3. INSERT INTO ... RETURNING nested in a WITH clause (CTE / Common Table Expression)
        // 		- Sqlite can only do RETURNING to the application, it cannot be nested in another query or diverted to another table
        // 		  https://sqlite.org/lang_returning.html#limitations_and_caveats
        // 4. CREATE TEMPORARY TABLE AS with an INSERT INTO ... RETURNING nested
        // 		- Same limitation AND CREATE TABLEAS seems to only support SELECT statements (but if we could nest RETURNING we can workaround that
        // 		  https://www.sqlite.org/lang_createtable.html#create_table_as_select_statements
        //
        // Hence we have to use row IDs and last_insert_rowid()
        //
        // Furthermore we use a view and an INSTEAD OF trigger to update the tables,
        // the alternative being
        //
        // 5. Direct insert into tables
        //		This does not work as SQLite `execute` and `prepare`
        //      only process the first statement.
        //
        // And lastly, we need the view and trigger to be temporary because
        // otherwise they can't access the temporary table:
        //   6. https://sqlite.org/forum/info/4f998eeec510bceee69404541e5c9ca0a301868d59ec7c3486ecb8084309bba1
        //      "Triggers in any schema other than temp may only access objects in their own schema. However, triggers in temp may access any object by name, even cross-schema."
        self.conn.execute_batch(
            r#"
            -- PRAGMA temp_store = 'MEMORY';
            CREATE TEMPORARY TABLE IF NOT EXISTS temp.current_task(task_id INTEGER);
            
            CREATE TEMPORARY TRIGGER IF NOT EXISTS enqueue_task_insert_trigger INSTEAD OF
            INSERT
              ON enqueue_task
            BEGIN
                INSERT INTO
                  tasks(chain_id, blockhash, proofsys_id, prover)
                VALUES
                  (
                    new.chain_id,
                    new.blockhash,
                    new.proofsys_id,
                    new.prover
                  );
                
                INSERT INTO
                  current_task
                SELECT
                  id
                FROM
                  tasks
                WHERE
                  rowid = last_insert_rowid()
                LIMIT
                  1;
                
                -- Tasks are initialized at status 1000 - registered
                -- timestamp is auto-filled with datetime('now'), see its field definition
                INSERT INTO
                  task_status(task_id, status_id)
                SELECT
                  tmp.task_id,
                  1000
                FROM
                  current_task tmp;
                
                DELETE FROM
                  current_task;
            END;
            
            CREATE TEMPORARY TRIGGER IF NOT EXISTS update_task_progress_trigger INSTEAD OF
            INSERT
              ON update_task_progress
            BEGIN
                INSERT INTO
                  current_task
                SELECT
                  id
                FROM
                  tasks
                WHERE
                  chain_id = new.chain_id
                  AND blockhash = new.blockhash
                  AND proofsys_id = new.proofsys_id
                LIMIT
                  1;
                
                -- timestamp is auto-filled with datetime('now'), see its field definition
                INSERT INTO
                  task_status(task_id, status_id)
                SELECT
                  tmp.task_id,
                  new.status_id
                FROM
                  current_task tmp
                LIMIT
                  1;
                
                INSERT
                  OR REPLACE INTO task_proofs
                SELECT
                  task_id,
                  new.proof
                FROM
                  current_task
                WHERE
                  new.proof IS NOT NULL
                LIMIT
                  1;
                
                DELETE FROM
                  current_task;
            END;
            "#,
        )?;

        Ok(())
    }

    pub fn enqueue_task(
        &self,
        TaskDescriptor {
            chain_id,
            blockhash,
            proof_system,
            prover,
        }: &TaskDescriptor,
    ) -> TaskManagerResult<Vec<TaskProvingStatus>> {
        let mut statement = self.conn.prepare_cached(
            r#"
            INSERT INTO
              enqueue_task(
                chain_id,
                blockhash,
                proofsys_id,
                prover
              )
            VALUES
              (
                :chain_id,
                :blockhash,
                :proofsys_id,
                :prover
              );
            "#,
        )?;
        statement.execute(named_params! {
            ":chain_id": chain_id,
            ":blockhash": blockhash.to_vec(),
            ":proofsys_id": *proof_system as u8,
            ":prover": prover,
        })?;

        Ok(vec![(
            TaskStatus::Registered,
            Some(prover.clone()),
            Utc::now(),
        )])
    }

    pub fn update_task_progress(
        &self,
        TaskDescriptor {
            chain_id,
            blockhash,
            proof_system,
            prover,
        }: TaskDescriptor,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        let mut statement = self.conn.prepare_cached(
            r#"
            INSERT INTO
              update_task_progress(
                chain_id,
                blockhash,
                proofsys_id,
                status_id,
                prover,
                proof
              )
            VALUES
              (
                :chain_id,
                :blockhash,
                :proofsys_id,
                :status_id,
                :prover,
                :proof
              );
            "#,
        )?;
        statement.execute(named_params! {
            ":chain_id": chain_id,
            ":blockhash": blockhash.to_vec(),
            ":proofsys_id": proof_system as u8,
            ":prover": prover,
            ":status_id": status as i32,
            ":proof": proof.map(hex::encode)
        })?;

        Ok(())
    }

    pub fn get_task_proving_status(
        &self,
        TaskDescriptor {
            chain_id,
            blockhash,
            proof_system,
            prover,
        }: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let mut statement = self.conn.prepare_cached(
            r#"
            SELECT
              ts.status_id,
              tp.proof,
              timestamp
            FROM
              task_status ts
              LEFT JOIN tasks t ON ts.task_id = t.id
              LEFT JOIN task_proofs tp ON tp.task_id = t.id
            WHERE
              t.chain_id = :chain_id
              AND t.blockhash = :blockhash
              AND t.proofsys_id = :proofsys_id
              AND t.prover = :prover
            ORDER BY
              ts.timestamp;
            "#,
        )?;
        let query = statement.query_map(
            named_params! {
                ":chain_id": chain_id,
                ":blockhash": blockhash.to_vec(),
                ":proofsys_id": *proof_system as u8,
                ":prover": prover,
            },
            |row| {
                Ok((
                    TaskStatus::from(row.get::<_, i32>(0)?),
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, DateTime<Utc>>(2)?,
                ))
            },
        )?;

        Ok(query.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn get_task_proof(
        &self,
        TaskDescriptor {
            chain_id,
            blockhash,
            proof_system,
            prover,
        }: &TaskDescriptor,
    ) -> TaskManagerResult<Vec<u8>> {
        let mut statement = self.conn.prepare_cached(
            r#"
            SELECT
              proof
            FROM
              task_proofs tp
              LEFT JOIN tasks t ON tp.task_id = t.id
            WHERE
              t.chain_id = :chain_id
              AND t.prover = :prover
              AND t.blockhash = :blockhash
              AND t.proofsys_id = :proofsys_id
            LIMIT
              1;
            "#,
        )?;
        let query = statement.query_row(
            named_params! {
                ":chain_id": chain_id,
                ":blockhash": blockhash.to_vec(),
                ":proofsys_id": *proof_system as u8,
                ":prover": prover,
            },
            |row| row.get::<_, Option<String>>(0),
        )?;

        let Some(proof) = query else {
            return Ok(vec![]);
        };

        hex::decode(proof)
            .map_err(|_| TaskManagerError::SqlError("couldn't decode from hex".to_owned()))
    }

    pub fn get_db_size(&self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        let mut statement = self.conn.prepare_cached(
            r#"
            SELECT
              name as table_name,
              SUM(pgsize) as table_size
            FROM
              dbstat
            GROUP BY
              table_name
            ORDER BY
              SUM(pgsize) DESC;
            "#,
        )?;
        let query = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        let details = query.collect::<Result<Vec<_>, _>>()?;
        let total = details.iter().fold(0, |acc, (_, size)| acc + size);

        Ok((total, details))
    }

    pub fn prune_db(&self) -> TaskManagerResult<()> {
        let mut statement = self.conn.prepare_cached(
            r#"
            DELETE FROM
              tasks;
            
            DELETE FROM
              task_proofs;
            
            DELETE FROM
              task_status;
            "#,
        )?;
        statement.execute([])?;

        Ok(())
    }

    pub fn list_all_tasks(&self) -> TaskManagerResult<Vec<TaskReport>> {
        let mut statement = self.conn.prepare_cached(
            r#"
            SELECT
              chain_id,
              blockhash,
              proofsys_id,
              prover,
              status_id
            FROM
              tasks
              LEFT JOIN task_status on task.id = task_status.task_id
              JOIN (
                SELECT
                  task_id,
                  MAX(timestamp) as latest_timestamp
                FROM
                  task_status
                GROUP BY
                  task_id
              ) latest_ts ON task_status.task_id = latest_ts.task_id
              AND task_status.timestamp = latest_ts.latest_timestamp
            "#,
        )?;
        let query = statement
            .query_map([], |row| {
                Ok((
                    TaskDescriptor {
                        chain_id: row.get(0)?,
                        blockhash: B256::from_slice(&row.get::<_, Vec<u8>>(1)?),
                        proof_system: row.get::<_, u8>(2)?.try_into().unwrap(),
                        prover: row.get(3)?,
                    },
                    TaskStatus::from(row.get::<_, i32>(4)?),
                ))
            })?
            .collect::<Result<Vec<TaskReport>, _>>()?;

        Ok(query)
    }

    fn store_id(
        &self,
        (chain_id, blockhash, proof_key): ProofKey,
        id: String,
    ) -> TaskManagerResult<()> {
        let mut statement = self.conn.prepare_cached(
            r#"
            INSERT INTO
              store(
                chain_id,
                blockhash,
                proofsys_id,
                id
              )
            VALUES
              (
                :chain_id,
                :blockhash,
                :proofsys_id,
                :id
              );
            "#,
        )?;
        statement.execute(named_params! {
            ":chain_id": chain_id,
            ":blockhash": blockhash.to_vec(),
            ":proofsys_id": proof_key as u8,
            ":id": id,
        })?;

        Ok(())
    }

    fn remove_id(&self, (chain_id, blockhash, proof_key): ProofKey) -> TaskManagerResult<()> {
        let mut statement = self.conn.prepare_cached(
            r#"
            DELETE FROM
              store
            WHERE
              chain_id = :chain_id
              AND blockhash = :blockhash
              AND proofsys_id = :proofsys_id;
            "#,
        )?;
        statement.execute(named_params! {
            ":chain_id": chain_id,
            ":blockhash": blockhash.to_vec(),
            ":proofsys_id": proof_key as u8,
        })?;

        Ok(())
    }

    fn read_id(&self, (chain_id, blockhash, proof_key): ProofKey) -> TaskManagerResult<String> {
        let mut statement = self.conn.prepare_cached(
            r#"
            SELECT
              id
            FROM
              store
            WHERE
              chain_id = :chain_id
              AND blockhash = :blockhash
              AND proofsys_id = :proofsys_id
            LIMIT
              1;
            "#,
        )?;
        let query = statement.query_row(
            named_params! {
                ":chain_id": chain_id,
                ":blockhash": blockhash.to_vec(),
                ":proofsys_id": proof_key as u8,
            },
            |row| row.get::<_, String>(0),
        )?;

        Ok(query)
    }
}

impl IdWrite for SqliteTaskManager {
    fn store_id(&mut self, key: ProofKey, id: String) -> ProverResult<()> {
        let rt = Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async move {
            let task_db = self.arc_task_db.lock().await;
            task_db.store_id(key, id)
        })
        .map_err(|e| ProverError::StoreError(e.to_string()))
    }

    fn remove_id(&mut self, key: ProofKey) -> ProverResult<()> {
        let rt = Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async move {
            let task_db = self.arc_task_db.lock().await;
            task_db.remove_id(key)
        })
        .map_err(|e| ProverError::StoreError(e.to_string()))
    }
}

impl IdStore for SqliteTaskManager {
    fn read_id(&self, key: ProofKey) -> ProverResult<String> {
        let rt = Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async move {
            let task_db = self.arc_task_db.lock().await;
            task_db.read_id(key)
        })
        .map_err(|e| ProverError::StoreError(e.to_string()))
    }
}

#[async_trait::async_trait]
impl TaskManager for SqliteTaskManager {
    fn new(opts: &TaskManagerOpts) -> Self {
        static INIT: Once = Once::new();
        static mut CONN: Option<Arc<Mutex<TaskDb>>> = None;
        INIT.call_once(|| {
            unsafe {
                CONN = Some(Arc::new(Mutex::new({
                    let db = TaskDb::open_or_create(&opts.sqlite_file).unwrap();
                    db.manage().unwrap();
                    db
                })))
            };
        });
        Self {
            arc_task_db: unsafe { CONN.clone().unwrap() },
        }
    }

    async fn enqueue_task(
        &mut self,
        params: &TaskDescriptor,
    ) -> Result<Vec<TaskProvingStatus>, TaskManagerError> {
        let task_db = self.arc_task_db.lock().await;
        task_db.enqueue_task(params)
    }

    async fn update_task_progress(
        &mut self,
        key: TaskDescriptor,
        status: TaskStatus,
        proof: Option<&[u8]>,
    ) -> TaskManagerResult<()> {
        let task_db = self.arc_task_db.lock().await;
        task_db.update_task_progress(key, status, proof)
    }

    /// Returns the latest triplet (submitter or fulfiller, status, last update time)
    async fn get_task_proving_status(
        &mut self,
        key: &TaskDescriptor,
    ) -> TaskManagerResult<TaskProvingStatusRecords> {
        let task_db = self.arc_task_db.lock().await;
        task_db.get_task_proving_status(key)
    }

    async fn get_task_proof(&mut self, key: &TaskDescriptor) -> TaskManagerResult<Vec<u8>> {
        let task_db = self.arc_task_db.lock().await;
        task_db.get_task_proof(key)
    }

    /// Returns the total and detailed database size
    async fn get_db_size(&mut self) -> TaskManagerResult<(usize, Vec<(String, usize)>)> {
        let task_db = self.arc_task_db.lock().await;
        task_db.get_db_size()
    }

    async fn prune_db(&mut self) -> TaskManagerResult<()> {
        let task_db = self.arc_task_db.lock().await;
        task_db.prune_db()
    }

    async fn list_all_tasks(&mut self) -> TaskManagerResult<Vec<TaskReport>> {
        let task_db = self.arc_task_db.lock().await;
        task_db.list_all_tasks()
    }
}

#[cfg(test)]
mod tests {
    // We only test private functions here.
    // Public API will be tested in a dedicated tests folder

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn error_on_missing() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("db.sqlite");
        assert!(TaskDb::open(&file).is_err());
    }

    #[test]
    fn ensure_exclusive() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("db.sqlite");

        let _db = TaskDb::create(&file).unwrap();
        assert!(TaskDb::open(&file).is_err());
        std::fs::remove_file(&file).unwrap();
    }

    #[test]
    fn ensure_unicity() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("db.sqlite");

        let _db = TaskDb::create(&file).unwrap();
        assert!(TaskDb::create(&file).is_err());
        std::fs::remove_file(&file).unwrap();
    }
}
