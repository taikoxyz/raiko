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
//! |   100       | Success but pruned               |
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
//! | id_task | chainID | block_number | blockhash | parentHash | stateRoot | # of txs | gas_used |
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

use rusqlite::{named_params, params, Statement};
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
    SuccessButPruned          =   100,
    WorkInProgress            =  1000,
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

    // Queries
    // ----------------------------------------------------------------

    fn create_tables(conn: &Connection) -> Result<(), TaskManagerError> {
        conn.execute(
            "CREATE TABLE metadata(
                key BLOB NOT NULL PRIMARY KEY,
                value BLOB
            )",
            params![],
        )?;
        conn.execute(
            "INSERT INTO
                metadata(key, value)
             VALUES
                (?, ?);",
            params!["task_db_version", 0u32],
        )?;

        conn.execute(
            "CREATE TABLE proofsys(
                id_proofsys INTEGER NOT NULL PRIMARY KEY,
                desc TEXT NOT NULL
            )",
            params![],
        )?;
        conn.execute(
            "INSERT INTO
                proofsys(id_proofsys, desc)
             VALUES
                (0, 'Risc0'),
                (1, 'SP1'),
                (2, 'SGX');",
            params![],
        )?;

        conn.execute(
            "CREATE TABLE status_codes(
                id_status INTEGER NOT NULL PRIMARY KEY,
                desc TEXT NOT NULL
            )",
            params![],
        )?;
        conn.execute(
            "INSERT INTO
                status_codes(id_status, desc)
             VALUES
                (    0, 'Success'),
                (  100, 'Success but pruned'),
                ( 1000, 'Work-in-progress'),
                (-1000, 'Proof failure (generic)'),
                (-1100, 'Proof failure (Out-Of-Memory)'),
                (-2000, 'Network failure'),
                (-3000, 'Cancelled'),
                (-3100, 'Cancelled (never started)'),
                (-3200, 'Cancelled (aborted)'),
                (-3210, 'Cancellation in progress'),
                (-4000, 'Invalid or unsupported block'),
                (-9999, 'Unspecified failure reason');",
            params![],
        )?;

        conn.execute(
            "CREATE TABLE proofs(
                id_proof INTEGER NOT NULL PRIMARY KEY,
                value BLOB NOT NULL
            )",
            params![],
        )?;

        // Notes:
        //   1. a blockhash may appear as many times as there are prover backends.
        //   2. For query speed over (chainID, blockhash, id_proofsys)
        //      there is no need to create an index as the UNIQUE constraint
        //      has an implied index, see:
        //      - https://sqlite.org/lang_createtable.html#uniqueconst
        //      - https://www.sqlite.org/fileformat2.html#representation_of_sql_indices
        conn.execute(
            "CREATE TABLE taskqueue(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                chainID INTEGER NOT NULL,
                blockhash BLOB NOT NULL,
                id_proofsys INTEGER NOT NULL,
                id_status INTEGER NOT NULL,
                id_proof INTEGER,
                FOREIGN KEY(chainID, blockhash) REFERENCES blocks(chainID, blockhash)
                FOREIGN KEY(id_proofsys) REFERENCES proofsys(id_proofsys)
                FOREIGN KEY(id_status) REFERENCES status_codes(id_status)
                FOREIGN KEY(id_proof) REFERENCES proofs(id_proof)
                UNIQUE (chainID, blockhash, id_proofsys)
            )",
            params![],
        )?;
        // Different blockchains might have the same blockhash in case of a fork
        // for example Ethereum and Ethereum Classic.
        // As "GuestInput" refers to ChainID, the proving task would be different.
        conn.execute(
            "CREATE TABLE blocks(
                chainID INTEGER NOT NULL,
                blockhash BLOB NOT NULL,
                block_number INTEGER NOT NULL,
                parentHash BLOB NOT NULL,
                stateRoot BLOB NOT NULL,
                num_transactions INTEGER NOT NULL,
                gas_used INTEGER NOT NULL,
                PRIMARY KEY (chainID, blockhash)
            )",
            params![],
        )?;
        // Payloads will be very large, 1.77MB on L1 in Jan 2024 (Before EIP-4844 blobs),
        //   https://ethresear.ch/t/on-block-sizes-gas-limits-and-scalability/18444
        // mandating ideally a separated high-performance KV-store to reduce IO.
        conn.execute(
            "CREATE TABLE task_payloads(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                payload BLOB NOT NULL,
                FOREIGN KEY(id_task) REFERENCES taskqueue(id_task)
            )",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE task_requests(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                submitter TEXT NOT NULL,
                submit_date TEXT NOT NULL,
                FOREIGN KEY(id_task) REFERENCES taskqueue(id_task)
            )",
            params![],
        )?;
        conn.execute(
            "CREATE TABLE task_fulfillment(
                id_task INTEGER PRIMARY KEY UNIQUE NOT NULL,
                fulfiller TEXT NOT NULL,
                fulfill_date TEXT NOT NULL,
                FOREIGN KEY(id_task) REFERENCES taskqueue(id_task)
            )",
            params![],
        )?;

        Ok(())
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
        // 5. Views + trigger on view inserts
        //		This introduces state beyond just the DB tables.
        //      Furthermore we would still need a transaction and last_insert_rowid() anyway
        //
        // Hence we have to use row IDs and last_insert_rowid()
        //
        // Now as a last boss, bindings via params! or named_params! is broken with multi-statements.
        // Only the first statement is taken into account.
        //
        // i.e if 2 INSERTs, only parameters from the first one are counted.
        // If DROP temp.table then INSERT, no parameters is counted.
        // If BEGIN TRANSACTION; then INSERT, no parameters is counted.
        //
        // Hence we require exclusive DB locking, single connection, single thread.
        //
        // Then we insert first in a temporary table.
        // That table must be created beforehand and cleared after each transaction,
        // so that the INSERT INTO is the very first statement.

        self.conn.execute_batch(
            "
            -- PRAGMA temp_store = 'MEMORY';
            DROP TABLE IF EXISTS temp.current_task;

            CREATE TEMPORARY TABLE temp.current_task(
                id_task INTEGER,
                chainID INTEGER,
                blockhash BLOB,
                id_proofsys INTEGER,
                id_status INTEGER,
                payload BLOB,
                submitter TEXT
            );
        ")?;

        let enqueue_task = self.conn.prepare(
            "
            INSERT INTO temp.current_task(chainID, blockhash, id_proofsys, id_status, payload, submitter)
                VALUES (:chainID, :blockhash, :id_proofsys, :id_status, :payload, :submitter);

            INSERT INTO taskqueue(chainID, blockhash, id_proofsys, id_status)
                SELECT chainID, blockhash, id_proofsys, id_status FROM temp.current_task;

            UPDATE temp.current_task
                SET id_task = last_insert_rowid();

            INSERT INTO task_payloads(id_task, payload)
                SELECT id_task, payload from temp.current_task
                LIMIT 1;

            INSERT INTO task_requests(id_task, submitter, submit_date)
                SELECT id_task, submitter, datetime('now') from temp.current_task
                LIMIT 1;

            DELETE FROM temp.current_task;
            ",
        ).unwrap();

        // println!("param count: {:?}", enqueue_task.parameter_count());

        // println!("chainID: {:?}", enqueue_task.parameter_index(":chainID"));
        // println!("blockhash: {:?}", enqueue_task.parameter_index(":blockhash"));
        // println!("id_proofsys: {:?}", enqueue_task.parameter_index(":id_proofsys"));
        // println!("id_status: {:?}", enqueue_task.parameter_index(":id_status"));
        // println!("payload: {:?}", enqueue_task.parameter_index(":payload"));
        // println!("submitter: {:?}", enqueue_task.parameter_index(":submitter"));

        // println!("example: {:?}", enqueue_task.parameter_index(":example"));

        Ok(TaskManager { enqueue_task })
    }
}

impl<'db> TaskManager<'db> {
    pub fn enqueue_task(
        &mut self,
        chain_id: ChainId,
        blockhash: B256,
        proof_system: TaskProofsys,
        payload: &[u8],
        submitter: &str,
    ) -> Result<(), TaskManagerError> {

        println!("{}", self.enqueue_task.expanded_sql().unwrap());

        let status = TaskStatus::WorkInProgress;

        self.enqueue_task.execute(named_params! {
            ":chainID": chain_id as u64,
            ":blockhash": blockhash.as_slice(),
            ":id_proofsys": proof_system as u8,
            ":id_status": status as u8,
            ":payload": payload,
            ":submitter": submitter,
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
