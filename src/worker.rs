use std::sync::mpsc::{self, Receiver, Sender};

use tokio::runtime::Runtime;

use crate::backends;
use crate::config::Connection;
use crate::models::{NodeMeta, QueryResult, TreeNode};

pub enum Job {
    TestConnection {
        conn: Connection,
    },
    ExpandNode {
        conn: Connection,
        node: TreeNode,
    },
    RunQuery {
        conn: Connection,
        query: String,
        context: NodeMeta,
        tab_id: u64,
    },
    Preview {
        conn: Connection,
        node: TreeNode,
        tab_id: u64,
    },
}

pub enum Event {
    Progress(String),
    Error(String),
    Busy(bool),
    TestOk {
        connection_id: String,
        message: String,
    },
    Children {
        connection_id: String,
        parent_id: String,
        children: Vec<TreeNode>,
    },
    QueryDone {
        tab_id: u64,
        result: QueryResult,
    },
}

pub struct Worker {
    tx: Sender<Event>,
    rt: Runtime,
}

impl Worker {
    pub fn spawn() -> (Self, Receiver<Event>) {
        let (tx, rx) = mpsc::channel();
        let rt = Runtime::new().expect("tokio runtime");
        (Self { tx, rt }, rx)
    }

    pub fn submit(&self, job: Job) {
        let tx = self.tx.clone();
        let _ = tx.send(Event::Busy(true));
        self.rt.spawn(async move {
            let result = run_job(job, tx.clone()).await;
            match result {
                Ok(Some(ev)) => {
                    let _ = tx.send(ev);
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = tx.send(Event::Error(format!("{e:#}")));
                }
            }
            let _ = tx.send(Event::Busy(false));
        });
    }
}

async fn run_job(job: Job, tx: Sender<Event>) -> anyhow::Result<Option<Event>> {
    match job {
        Job::TestConnection { conn } => {
            let _ = tx.send(Event::Progress(format!("测试连接 {}…", conn.name)));
            let message = backends::test_connection(&conn).await?;
            Ok(Some(Event::TestOk {
                connection_id: conn.id,
                message,
            }))
        }
        Job::ExpandNode { conn, node } => {
            let _ = tx.send(Event::Progress(format!("加载 {}…", node.label)));
            let parent_id = node.id.clone();
            let connection_id = conn.id.clone();
            let children = backends::list_children(&conn, &node).await?;
            Ok(Some(Event::Children {
                connection_id,
                parent_id,
                children,
            }))
        }
        Job::RunQuery {
            conn,
            query,
            context,
            tab_id,
        } => {
            let _ = tx.send(Event::Progress("执行查询…".into()));
            let result = backends::run_query(&conn, &query, &context).await?;
            Ok(Some(Event::QueryDone { tab_id, result }))
        }
        Job::Preview {
            conn,
            node,
            tab_id,
        } => {
            let _ = tx.send(Event::Progress(format!("打开 {}…", node.label)));
            let result = backends::preview_object(&conn, &node).await?;
            Ok(Some(Event::QueryDone { tab_id, result }))
        }
    }
}
