mod database;

fn main() -> rusqlite::Result<()> {
    println!(
        "Welcome to OPON!
Your productivity, your knowledge, your workspace."
    );

    let conn = database::initialize()?;

    println!("\nDatabase initialized successfully!");

    database::add_note(
        &conn,
        "First Note",
        "Testing notes content",
    )?;

    println!("\nNote added successfully!");

    let notes = database::get_notes(&conn)?;

    println!("\nNOTES RETRIEVED:");
    for note in &notes {
        println!("{:#?}", note);
    }

    if let Some(note) = notes.first() {
        database::update_note(
            &conn,
            note.id,
            "Updated First Note",
            "This content has been updated!",
        )?;

        println!("\nNote updated successfully!");
    }

    let notes = database::get_notes(&conn)?;

    println!("\nNOTES AFTER UPDATE:");
    for note in &notes {
        println!("{:#?}", note);
    }

    if let Some(note) = notes.first() {
        database::delete_note(&conn, note.id)?;

        println!("\nNote deleted successfully!");
    }

    let notes = database::get_notes(&conn)?;

    println!("\nNOTES AFTER DELETE:");
    for note in &notes {
        println!("{:#?}", note);
    }

    println!("\nOPON database tests completed!");

    Ok(())
}