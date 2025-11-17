use actix_cors::Cors;
use actix_web::{get, web, App, HttpServer, Responder, Result, error, HttpResponse, http::header};
use rusqlite::Connection;
use serde::Serialize;
use std::env;

// The Item struct remains the same
#[derive(Serialize)]
struct SendingUnitItem {
    id: u32,
    tower_fid: u32,
    cell_type: String,
    mount_height: Option<f64>,       // <-- changed
    mount_direction: Option<f64>,    // <-- changed
    safety_distance: f64,
    vertical_safety_distance: f64,
}

#[derive(Serialize)]
struct TowerItem {
    fid: u32,
    latitude: f64,
    longitude: f64,
    creation_date: String,
    provider_telekom: bool,
    provider_vodafone: bool,
    provider_telefonica: bool,
    provider_1und1: bool,
}

#[derive(Serialize)]
struct TowerWithUnits {
    #[serde(flatten)]
    tower: TowerItem,
    units: Vec<SendingUnitItem>,
}

// --- Health Check Endpoint ---
#[get("/health")]
async fn health_check(db_path: web::Data<String>) -> impl Responder {
    let db_ok = web::block(move || {
        let Ok(conn) = Connection::open(db_path.as_str()) else {
            return false;
        };
        conn.query_row("SELECT 1", [], |_| Ok(())).is_ok()
    })
    .await;

    match db_ok {
        Ok(true) => HttpResponse::Ok().body("OK"),
        _ => HttpResponse::InternalServerError().body("Database connection failed"),
    }
}

// The handler function is now more complex
#[get("/towers/{id}")]
async fn get_tower_details(
    id: web::Path<u32>,
    db_path: web::Data<String>,
) -> Result<impl Responder> {
    let tower_fid = id.into_inner();
    let path = db_path.get_ref().clone();

    // web::block runs blocking code in a thread pool
    let tower_with_units = web::block(move || {
        // Open a new connection in the new thread.
        let conn = Connection::open(path)?;

        let tower = conn.query_row(
            "SELECT * FROM towers WHERE fid = ?1",
            [tower_fid],
            |row| {
                // Try to get the creation_date as Option<String>
                let creation_date: Option<String> = row.get(3)?;
                Ok(TowerItem {
                    fid: row.get(0)?,
                    latitude: row.get(1)?,
                    longitude: row.get(2)?,
                    creation_date: creation_date.unwrap_or_else(|| "unbekannt".to_string()),
                    provider_telekom: row.get(4)?,
                    provider_vodafone: row.get(5)?,
                    provider_telefonica: row.get(6)?,
                    provider_1und1: row.get(7)?,
                })
            }
        )?;


        let mut stmt = conn.prepare("SELECT id, tower_fid, cell_type, mount_height, mount_direction, safety_distance, vertical_safety_distance FROM sending_units WHERE tower_fid = ?1")?;

        // 2. Query the database and map the rows to our Item struct
        let unit_iter = stmt.query_map([tower_fid], |row| {
            Ok(SendingUnitItem {
                id: row.get(0)?,
                tower_fid: row.get(1)?,
                cell_type: row.get(2)?,
                mount_height: row.get(3)?,
                mount_direction: row.get(4)?,
                safety_distance: row.get(5)?,
                vertical_safety_distance: row.get(6)?,
            })
        })?;

        // // 3. Collect the results into a Vec<Item>
        // let mut result_vec = Vec::new();
        // for item in unit_iter {
        //     result_vec.push(item?);
        // }
        let units = unit_iter.collect::<Result<Vec<_>, _>>()?;


        Ok(TowerWithUnits { tower, units })

    })
    .await
    .map_err(|e| error::ErrorInternalServerError(e.to_string()))? // Thread pool error
    .map_err(|e: rusqlite::Error| match e { // Database error
        rusqlite::Error::QueryReturnedNoRows => error::ErrorNotFound(format!("Tower with id {} not found", tower_fid)),
        _ => error::ErrorInternalServerError(e.to_string()),
    })?;

    Ok(web::Json(tower_with_units))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let db_path = env::var("DATABASE_URL").expect("DATABASE_URL must be set");

    println!("🚀 Server starting at http://0.0.0.0:8080");
    println!("📖 Using database at: {}", db_path);

    HttpServer::new(move || {

        let cors = Cors::default()
            .allow_any_origin() // Allows requests from any domain
            .allowed_methods(vec!["GET", "POST"]) // allowed methods
            .allowed_headers(vec![header::AUTHORIZATION, header::ACCEPT])
            .allowed_header(header::CONTENT_TYPE)
            .max_age(3600);

        App::new()
            .wrap(cors) // <-- 3. APPLY THE MIDDLEWARE
            .app_data(web::Data::new(db_path.clone()))
            .service(health_check)
            .service(get_tower_details)
    })
    .bind(("0.0.0.0", 8080))?
    .run()
    .await
}
