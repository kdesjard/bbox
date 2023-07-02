use crate::qwc2_config::*;
use actix_web::{get, web, Error, HttpRequest, HttpResponse};
use bbox_core::endpoints::abs_req_baseurl;
use bbox_core::static_files::EmbedFile;
use bbox_core::templates::{create_env_embedded, render_endpoint};
use bbox_map_server::inventory::{Inventory, WmsService};
use minijinja::{context, Environment};
use once_cell::sync::Lazy;
use rust_embed::RustEmbed;
use std::path::PathBuf;

#[derive(RustEmbed)]
#[folder = "templates/"]
struct Templates;

static TEMPLATES: Lazy<Environment<'static>> = Lazy::new(|| create_env_embedded(&Templates));

async fn index(inventory: web::Data<Inventory>) -> Result<HttpResponse, Error> {
    let template = TEMPLATES
        .get_template("index.html")
        .expect("couln't load template `index.html`");
    let links = vec![
        "/qgis/helloworld?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&BBOX=-67.593,-176.248,83.621,182.893&CRS=EPSG:4326&WIDTH=515&HEIGHT=217&LAYERS=Country,Hello&STYLES=,&FORMAT=image/png; mode=8bit&DPI=96&TRANSPARENT=TRUE",
        "/qgis/ne?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&BBOX=-20037508.34278924391,-5966981.031407224014,19750246.20310878009,17477263.06060761213&CRS=EPSG:900913&WIDTH=1399&HEIGHT=824&LAYERS=country&STYLES=&FORMAT=image/png; mode=8bit",
        "/wms/map/ne?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&BBOX=-20037508.34278924391,-5966981.031407224014,19750246.20310878009,17477263.06060761213&CRS=EPSG:900913&WIDTH=1399&HEIGHT=824&LAYERS=country&STYLES=&FORMAT=image/png; mode=8bit",
        "/wms/mock/helloworld?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&BBOX=-67.593,-176.248,83.621,182.893&CRS=EPSG:4326&WIDTH=515&HEIGHT=217&LAYERS=Country,Hello&STYLES=,&FORMAT=image/png; mode=8bit&DPI=96&TRANSPARENT=TRUE",
        ]
    ;
    let index = template
        .render(
            context!(cur_menu => "Maps", wms_services => &inventory.wms_services, links => links),
        )
        .expect("index.hmtl render failed");
    Ok(HttpResponse::Ok().content_type("text/html").body(index))
}

#[derive(RustEmbed)]
#[folder = "static/"]
struct Statics;

#[get("/favicon.ico")]
async fn favicon() -> Result<EmbedFile, Error> {
    Ok(EmbedFile::open(&Statics, PathBuf::from("favicon.ico"))?)
}

async fn qwc2_viewer(filename: web::Path<PathBuf>) -> Result<EmbedFile, Error> {
    qwc2_assets(&filename)
}

async fn qwc2_map(path: web::Path<(String, PathBuf)>) -> Result<EmbedFile, Error> {
    // Used for /qwc2_map/{theme}/index.html and /qwc2_map/{theme}/config.json
    qwc2_assets(&path.1)
}

fn qwc2_assets(filename: &PathBuf) -> Result<EmbedFile, Error> {
    let filename = if filename == &PathBuf::from("") {
        PathBuf::from("index.html")
    } else {
        filename.to_path_buf()
    };
    Ok(EmbedFile::open(
        &Statics,
        PathBuf::from("qwc2").join(filename),
    )?)
}

async fn qwc2_themes(
    inventory: web::Data<Inventory>,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    let json = themes_json(&inventory.wms_services, abs_req_baseurl(&req), None).await;
    Ok(HttpResponse::Ok().json(json))
}

async fn qwc2_theme(
    id: web::Path<String>,
    inventory: web::Data<Inventory>,
    req: HttpRequest,
) -> Result<HttpResponse, Error> {
    // let wms_service = inventory.wms_services.iter().find(|wms| wms.id == *id).unwrap().clone();
    let json = themes_json(&inventory.wms_services, abs_req_baseurl(&req), Some(&*id)).await;
    Ok(HttpResponse::Ok().json(json))
}

async fn themes_json(
    wms_services: &Vec<WmsService>,
    base_url: String,
    default_theme: Option<&str>,
) -> ThemesJson {
    let mut caps = Vec::new();
    for wms in wms_services {
        caps.push((wms, wms.capabilities(&base_url).await, wms.url(&base_url)));
    }
    ThemesJson::from_capabilities(caps, default_theme)
}

async fn maplibre(path: web::Path<PathBuf>) -> Result<EmbedFile, Error> {
    Ok(EmbedFile::open(
        &Statics,
        PathBuf::from("maplibre").join(path.as_ref()),
    )?)
}

async fn ol(path: web::Path<PathBuf>) -> Result<EmbedFile, Error> {
    Ok(EmbedFile::open(
        &Statics,
        PathBuf::from("ol").join(path.as_ref()),
    )?)
}

async fn proj(path: web::Path<PathBuf>) -> Result<EmbedFile, Error> {
    Ok(EmbedFile::open(
        &Statics,
        PathBuf::from("proj").join(path.as_ref()),
    )?)
}

async fn swagger(path: web::Path<PathBuf>) -> Result<EmbedFile, Error> {
    Ok(EmbedFile::open(
        &Statics,
        PathBuf::from("swagger").join(path.as_ref()),
    )?)
}

async fn swaggerui_html() -> Result<HttpResponse, Error> {
    render_endpoint(&TEMPLATES, "swaggerui.html", context!(cur_menu=>"API")).await
}

async fn redoc(path: web::Path<PathBuf>) -> Result<EmbedFile, Error> {
    Ok(EmbedFile::open(
        &Statics,
        PathBuf::from("redoc").join(path.as_ref()),
    )?)
}

async fn redoc_html() -> Result<HttpResponse, Error> {
    render_endpoint(&TEMPLATES, "redoc.html", context!(cur_menu=>"API")).await
}

pub fn register(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/").route(web::get().to(index)))
        .service(favicon)
        .service(web::resource("/qwc2/themes.json").route(web::get().to(qwc2_themes)))
        .service(web::resource(r#"/qwc2/{filename:.*}"#).route(web::get().to(qwc2_viewer)))
        .service(web::resource("/qwc2_map/{id}/themes.json").route(web::get().to(qwc2_theme)))
        .service(web::resource(r#"/qwc2_map/{id}/{filename:.*}"#).route(web::get().to(qwc2_map)))
        .service(web::resource(r#"/maplibre/{filename:.*}"#).route(web::get().to(maplibre)))
        .service(web::resource(r#"/ol/{filename:.*}"#).route(web::get().to(ol)))
        .service(web::resource(r#"/proj/{filename:.*}"#).route(web::get().to(proj)))
        .service(web::resource(r#"/swagger/{filename:.*}"#).route(web::get().to(swagger)))
        .service(web::resource("/swaggerui.html").route(web::get().to(swaggerui_html)))
        .service(web::resource(r#"/redoc/{filename:.*}"#).route(web::get().to(redoc)))
        .service(web::resource("/redoc.html").route(web::get().to(redoc_html)));
}