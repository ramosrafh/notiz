fn main() -> Result<(), Box<dyn std::error::Error>> {
    let services = notiz_services::AppServices::open_default()?;
    notiz_ui::run(services)?;
    Ok(())
}
