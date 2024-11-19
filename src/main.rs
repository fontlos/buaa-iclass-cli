use buaa_api::{Session, IClassCourse};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use time::{OffsetDateTime, PrimitiveDateTime, Time, UtcOffset};
use tokio::time::Duration;

use std::fs::{File, OpenOptions};

#[derive(Debug, Parser)]
#[command(
    version = "0.1.0",
    about = "A cli for BUAA IClass",
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Login to IClass.
    /// Username and Password can be saved in the configuration file, and you can also specify them here.
    Login {
        #[arg(short, long)]
        username: Option<String>,
        #[arg(short, long)]
        password: Option<String>,
    },
    /// List and manage courses.
    List {
        #[arg(short, long)]
        /// Remove course by ID.
        /// Because some courses may be invalid.
        remove: Option<String>,
    },
    /// Query courses and schedules.
    Query {
        #[arg(short, long)]
        /// Query term's courses by term ID. eg. '202420251' is autumn term of 2024.
        /// The result will be saved and you can use `list` command to use it.
        term: Option<String>,
        #[arg(short, long)]
        /// Query course's schedule by course ID
        course: Option<String>,
    },
    /// Checkin to a schedule. Support timed checkin.
    Checkin {
        #[arg(short, long)]
        /// Checkin by schedule ID directly.
        schedule: Option<String>,
        #[arg(short, long)]
        /// Checkin by course ID and you need to set time.
        course: Option<String>,
        #[arg(short, long)]
        /// eg. '0800' means 8:00.
        time: Option<String>,
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Config {
    username: String,
    password: String,
    user_id: String,
    courses: Vec<IClassCourse>,
}

fn main() {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open("buaa-iclass-config.json")
        .unwrap();
    let mut config = match serde_json::from_reader::<File, Config>(file){
        Ok(config) => config,
        Err(_) => Config::default(),
    };
    let mut session = Session::new_in_file("buaa-iclass-cookie.json");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let cli = Cli::parse();
    match cli.command {
        Commands::Login { username, password } => {
            if let Some(username) = username {
                config.username = username;
            }
            if let Some(password) = password {
                config.password = password;
            }
            runtime.block_on(async {
                match session.sso_login(&config.username, &config.password).await {
                    Ok(_) => println!("[Info]: SSO Login successfully"),
                    Err(e) => eprintln!("[Info]: SSO Login failed: {:?}", e),
                }
                let id = match session.iclass_login().await {
                    Ok(s) => {
                        println!("[Info]: Iclass Login successfully");
                        s
                    },
                    Err(e) => {
                        eprintln!("[Info]: Iclass Login failed: {:?}", e);
                        return;
                    },
                };
                config.user_id = id;
            });
        },
        Commands::List { remove } => {
            if let Some(id) = remove {
                config.courses.retain(|course| course.id != id);
            } else {
                let table = buaa_api::utils::table(&config.courses);
                println!("{}", table);
            }
        },
        Commands::Query { term, course } => {
            if let Some(term) = term {
                runtime.block_on(async {
                    let courses = match session.iclass_query_course(&term, &config.user_id).await {
                        Ok(courses) => courses,
                        Err(e) => {
                            eprintln!("[Info]: Query course failed: {:?}", e);
                            return;
                        },
                    };
                    let table = buaa_api::utils::table(&courses);
                    println!("{}", table);
                    config.courses = courses;
                });
            }
            if let Some(course) = course {
                runtime.block_on(async {
                    let schedule = match session.iclass_query_schedule(&course, &config.user_id).await {
                        Ok(schedule) => schedule,
                        Err(e) => {
                            eprintln!("[Info]: Query schedule failed: {:?}", e);
                            return;
                        },
                    };
                    let table = buaa_api::utils::table(&schedule);
                    println!("{}", table);
                });
            }
        },
        Commands::Checkin { schedule, course, time } => {
            if let Some(schedule) = schedule {
                runtime.block_on(async {
                    match session.iclass_checkin_schedule(&schedule, &config.user_id).await {
                        Ok(_) => println!("[Info]: Checkin successfully"),
                        Err(e) => eprintln!("[Info]: Checkin failed: {:?}", e),
                    }
                });
            }
            if let Some(course) = course {
                if let Some(time) = time {
                    let hour = time[0..2].parse::<u8>().unwrap();
                    let minute = time[2..4].parse::<u8>().unwrap();
                    let time = Time::from_hms(hour, minute, 0).unwrap();
                    let now = get_primitive_time();
                    let target = PrimitiveDateTime::new(now.date(), time);
                    let duration = target - now;
                    let second = duration.whole_seconds() + 5;
                    // 如果时间大于零那么就等待
                    if second > 0 {
                        let duration = Duration::from_secs(second as u64);
                        println!("[Info]: Waiting for {} seconds", second);
                        runtime.block_on(async {
                            tokio::time::sleep(duration).await;
                            let schedule = match session.iclass_query_schedule(&course, &config.user_id).await {
                                Ok(schedule) => schedule,
                                Err(e) => {
                                    eprintln!("[Info]: Query schedule failed: {:?}", e);
                                    return;
                                },
                            };
                            let schedule = schedule.last().unwrap();
                            match session.iclass_checkin_schedule(&schedule.id, &config.user_id).await {
                                Ok(_) => println!("[Info]: Checkin successfully"),
                                Err(e) => eprintln!("[Info]: Checkin failed: {:?}", e),
                            }
                        })
                    }
                }
            }
        }
    }
    session.save();
    let file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open("buaa-iclass-config.json")
        .unwrap();
    serde_json::to_writer(file, &config).unwrap();
}

fn get_primitive_time() -> PrimitiveDateTime {
    let now_utc = OffsetDateTime::now_utc();
    let local_offset = UtcOffset::from_hms(8, 0, 0).unwrap();
    let now_local = now_utc.to_offset(local_offset);
    PrimitiveDateTime::new(now_local.date(), now_local.time())
}