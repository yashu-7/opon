mod database;

fn main() -> rusqlite::Result<()> {
    println!(
        "Welcome to OPON!
Your productivity, your knowledge, your workspace."
    );

    let conn = database::initialize()?;

    println!("Database initialized successfully!");

    database::add_note(
        &conn,
        "First Note",
        "Testing notes content",
    )?;

    println!("Note added successfully!");

    let notes = database::get_notes(&conn);

    println!("NOTES RETRIVED: {:#?}", notes);

    Ok(())
}