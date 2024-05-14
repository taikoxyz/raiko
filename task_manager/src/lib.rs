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
//! - Prefixed with rts_ in-case the DB is co-located with other services.
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
//! |  1000       | Work-in-progress                 |
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
//!  ____________________________
//! | Proof cache               | A map: ID -> proof
//! |___________________________|
//! | id_proof | proof_value    |
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
//!
//!  _____________________________________________________________________________________________
//! | Tasks metadata                                                                              |
//! |_____________________________________________________________________________________________|
//! | id_task | chain_id | block_number | blockhash | parent_hash | state_root | # of txs | gas_used |
//! |_________|_________|______________|___________|____________|___________|__________|__________|
//!  ___________________________________________________________
//! | Task queue                                               |
//! |__________________________________________________________|
//! | id_task | blockhash | id_proofsys | id_status | id_proof |
//! |_________|___________|_____________|___________|__________|
//!  ______________________________________
//! | Tasks inputs                        |
//! |_____________________________________|
//! | id_task | inputs (serialized)       |
//! |_________|___________________________|
//!  _____________________________________
//! | Task requests                      |
//! |____________________________________|
//! | id_task | id_submitter | submit_dt |
//! |_________|______________|___________|
//!  ______________________________________
//! | Task fulfillment                   |
//! |_____________________________________|
//! | id_task | id_fulfiller | fulfill_dt |
//! |_________|______________|____________|
//!
//! Rationale:
//! - When dealing with proof requests we don't need to touch the fullfillment table
//! - and inversely when dealing with provers, we don't need to deal with the request table.
//! - inputs are very large and warrant a dedicated table, with pruning
//! - metadata is useful to audit block building and prover efficiency

// Imports
// ----------------------------------------------------------------
use rusqlite::Error as SqlError;
use std::io::{Error as IOError, ErrorKind as IOErrorKind};

use std::fs::File;
use std::path::Path;

use raiko_primitives::{BlockNumber, ChainId, B256};

use rusqlite::{named_params, Statement};
use rusqlite::{Connection, OpenFlags};

// Types
// ----------------------------------------------------------------

#[derive(PartialEq, Debug)]
pub enum TaskManagerError {
    IOError(IOErrorKind),
    SqlError(String),
}

impl From<IOError> for TaskManagerError {
    fn from(error: IOError) -> TaskManagerError {
        TaskManagerError::IOError(error.kind())
    }
}

impl From<SqlError> for TaskManagerError {
    fn from(error: SqlError) -> TaskManagerError {
        TaskManagerError::SqlError(error.to_string())
    }
}

#[derive(Debug)]
pub struct TaskDb {
    conn: Connection,
}

#[derive(Debug)]
pub struct TaskManager<'db> {
    enqueue_task: Statement<'db>,
    // dequeue_task: Statement<'db>,
    // get_block_proof_status: Statement<'db>,
}

pub enum TaskProofsys {
    Risc0 = 0,
    SP1 = 1,
    SGX = 2,
}

#[allow(non_camel_case_types)]
#[rustfmt::skip]
pub enum TaskStatus {
    Success                   =     0,
    Registered                =  1000,
    WorkInProgress            =  2000,
    ProofFailure_Generic      = -1000,
    ProofFailure_OutOfMemory  = -1100,
    NetworkFailure            = -2000,
    Cancelled                 = -3000,
    Cancelled_NeverStarted    = -3100,
    Cancelled_Aborted         = -3200,
    CancellationInProgress    = -3210,
    InvalidOrUnsupportedBlock = -4000,
    UnspecifiedFailureReason  = -9999,
}

// Implementation
// ----------------------------------------------------------------

impl TaskDb {
    fn open(path: &Path) -> Result<Connection, TaskManagerError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        Ok(conn)
    }

    fn create(path: &Path) -> Result<Connection, TaskManagerError> {
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
    pub fn open_or_create(path: &Path) -> Result<Self, TaskManagerError> {
        let conn = if path.exists() {
            Self::open(path)
        } else {
            Self::create(path)
        }?;
        Ok(Self { conn })
    }

    // SQL
    // ----------------------------------------------------------------

    fn create_tables(conn: &Connection) -> Result<(), TaskManagerError> {
        // Change the task_db_version if backward compatibility is broken
        // and introduce a migration on DB opening ... if conserving history is important.
        conn.execute_batch(
            r#"
            -- Metadata and mappings
            -----------------------------------------------

            CREATE TABLE metadata(
                key BLOB NOT NULL PRIMARY KEY,
                value BLOB
            );

            INSERT INTO
                metadata(key, value)
            VALUES
                ('task_db_version', 0);

            CREATE TABLE proofsys(
                id_proofsys INTEGER NOT NULL PRIMARY KEY,
                desc TEXT NOT NULL
            );

            INSERT INTO
                proofsys(id_proofsys, desc)
            VALUES
                (0, 'Risc0'),
                (1, 'SP1'),
                (2, 'SGX');

            CREATE TABLE status_codes(
                id_status INTEGER NOT NULL PRIMARY KEY,
                desc TEXT NOT NULL
            );

            INSERT INTO
                status_codes(id_status, desc)
            VALUES
                (    0, 'Success'),
                ( 1000, 'Registered'),
                ( 2000, 'Work-in-progress'),
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

            CREATE TABLE proofs(
                id_proof INTEGER NOT NULL PRIMARY KEY,
                value BLOB NOT NULL
            );

            -- Notes:
            --   1. a blockhash may appear as many times as there are prover backends.
            --   2. For query speed over (chain_id, blockhash, id_proofsys)
            --      there is no need to create an index as the UNIQUE constraint
            --      has an implied index, see:
            --      - https://sqlite.org/lang_createtable.html#uniqueconst
            --      - https://www.sqlite.org/fileformat2.html#representation_of_sql_indices
            CREATE TABLE taskqueue(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                chain_id INTEGER NOT NULL,
                blockhash BLOB NOT NULL,
                id_proofsys INTEGER NOT NULL,
                id_status INTEGER NOT NULL,
                id_proof INTEGER,
                FOREIGN KEY(chain_id, blockhash) REFERENCES blocks(chain_id, blockhash)
                FOREIGN KEY(id_proofsys) REFERENCES proofsys(id_proofsys)
                FOREIGN KEY(id_status) REFERENCES status_codes(id_status)
                FOREIGN KEY(id_proof) REFERENCES proofs(id_proof)
                UNIQUE (chain_id, blockhash, id_proofsys)
            );

            -- Different blockchains might have the same blockhash in case of a fork
            -- for example Ethereum and Ethereum Classic.
            -- As "GuestInput" refers to ChainID, the proving task would be different.
            CREATE TABLE blocks(
                chain_id INTEGER NOT NULL,
                blockhash BLOB NOT NULL,
                block_number INTEGER NOT NULL,
                parent_hash BLOB NOT NULL,
                state_root BLOB NOT NULL,
                num_transactions INTEGER NOT NULL,
                gas_used INTEGER NOT NULL,
                PRIMARY KEY (chain_id, blockhash)
            );

            -- Payloads will be very large, just the block would be 1.77MB on L1 in Jan 2024,
            --   https://ethresear.ch/t/on-block-sizes-gas-limits-and-scalability/18444
            -- mandating ideally a separated high-performance KV-store to reduce IO.
            -- This is without EIP-4844 blobs and the extra input for zkVMs.
            CREATE TABLE task_payloads(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                payload BLOB NOT NULL,
                FOREIGN KEY(id_task) REFERENCES taskqueue(id_task)
            );

            CREATE TABLE task_requests(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                submitter TEXT NOT NULL,
                submit_date TEXT NOT NULL,
                FOREIGN KEY(id_task) REFERENCES taskqueue(id_task)
            );

            CREATE TABLE task_fulfillment(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                fulfiller TEXT NOT NULL,
                fulfill_date TEXT NOT NULL,
                FOREIGN KEY(id_task) REFERENCES taskqueue(id_task)
            );
            "#)?;

        Ok(())
    }

    fn create_views(conn: &Connection) -> Result<(), TaskManagerError> {
        // By convention, views will use an action verb as name.
        conn.execute_batch(
            r#"
            CREATE VIEW enqueue_task AS
                SELECT
                    tq.id_task,
                    tq.chain_id,
                    tq.blockhash,
                    tq.id_proofsys,
                    tq.id_status,
                    tr.submitter,
                    b.block_number,
                    b.parent_hash,
                    b.state_root,
                    b.num_transactions,
                    b.gas_used,
                    tp.payload
                FROM
                    taskqueue tq
                    LEFT JOIN
                        blocks b on (
                            b.chain_id = tq.chain_id
                            AND b.blockhash = tq.blockhash
                        )
                    LEFT JOIN
                        task_payloads tp on tp.id_task = tq.id_task
                    LEFT JOIN
                        task_requests tr on tr.id_task = tq.id_task;
            "#)?;

        Ok(())
    }

    /// Set a tracer to debug SQL execution
    /// for example:
    ///   db.set_tracer(Some(|stmt| println!("sqlite:\n-------\n{}\n=======", stmt)));
    #[cfg(test)]
    pub fn set_tracer(&mut self, trace_fn: Option<fn(_: &str)>) {
        self.conn.trace(trace_fn);
    }

    pub fn manage<'db>(&'db self) -> Result<TaskManager<'db>, TaskManagerError> {
        // To update all the tables with the task_id assigned by Sqlite
        // we require row IDs for the taskqueue table
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

        let conn = &self.conn;
        conn.execute_batch(
            "
            -- PRAGMA temp_store = 'MEMORY';

            CREATE TEMPORARY TABLE temp.current_task(id_task INTEGER);

            CREATE TEMPORARY TRIGGER enqueue_task_insert_trigger
                INSTEAD OF INSERT ON enqueue_task
                BEGIN
                    INSERT INTO blocks(chain_id, blockhash, block_number, parent_hash, state_root, num_transactions, gas_used)
                        VALUES (new.chain_id, new.blockhash, new.block_number, new.parent_hash, new.state_root, new.num_transactions, new.gas_used);

                    -- Tasks are initialized at status 1000 - registered
                    INSERT INTO taskqueue(chain_id, blockhash, id_proofsys, id_status)
                        VALUES (new.chain_id, new.blockhash, new.id_proofsys, 1000);

                    INSERT INTO current_task
                        SELECT id_task FROM taskqueue
                        WHERE rowid = last_insert_rowid()
                        LIMIT 1;

                    INSERT INTO task_payloads(id_task, payload)
                        SELECT tmp.id_task, new.payload
                        FROM current_task tmp
                        LIMIT 1;

                    INSERT INTO task_requests(id_task, submitter, submit_date)
                        SELECT tmp.id_task, new.submitter, datetime('now')
                        FROM current_task tmp
                        LIMIT 1;

                    DELETE FROM current_task;
                END;
            ")?;

        let enqueue_task = conn.prepare(
            "
            INSERT INTO enqueue_task(
                    chain_id, blockhash, id_proofsys, submitter,
                    block_number, parent_hash, state_root, num_transactions, gas_used,
                    payload)
                VALUES (
                    :chain_id, :blockhash, :id_proofsys, :submitter,
                    :block_number, :parent_hash, :state_root, :num_transactions, :gas_used,
                    :payload);
            ")?;

        Ok(TaskManager { enqueue_task })
    }
}

impl<'db> TaskManager<'db> {
    pub fn enqueue_task(
        &mut self,
        chain_id: ChainId,
        blockhash: &B256,
        proof_system: TaskProofsys,
        submitter: &str,
        block_number: BlockNumber,
        parent_hash: &B256,
        state_root: &B256,
        num_transactions: u64,
        gas_used: u64,
        payload: &[u8],
    ) -> Result<(), TaskManagerError> {
        self.enqueue_task.execute(named_params! {
            ":chain_id": chain_id as u64,
            ":blockhash": blockhash.as_slice(),
            ":id_proofsys": proof_system as u8,
            ":submitter": submitter,
            ":block_number": block_number,
            ":parent_hash": parent_hash.as_slice(),
            ":state_root": state_root.as_slice(),
            ":num_transactions": num_transactions,
            ":gas_used": gas_used,
            ":payload": payload,
        })?;
        Ok(())
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
    }

    #[test]
    fn ensure_unicity() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("db.sqlite");

        let _db = TaskDb::create(&file).unwrap();
        assert!(TaskDb::create(&file).is_err());
    }
}
