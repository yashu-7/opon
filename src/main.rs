mod database;
mod repository;

use repository::NoteRepository;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "Welcome to OPON!
Your productivity, your knowledge, your workspace."
    );

    let conn = database::initialize()?;

    println!("\nDatabase initialized successfully!");

    let repository = NoteRepository::new(conn);

    // CREATE
    let note_id = repository.add_note(
        "First Note",
        "Testing notes content",
    )?;

    println!("\nNote added successfully! ID: {}", note_id);

    // READ
    let notes = repository.get_notes()?;

    println!("\nNOTES RETRIEVED:");

    for note in &notes {
        println!("{:#?}", note);
    }

    // UPDATE
    let rows_updated = repository.update_note(
        note_id,
        "Updated First Note",
        "This content has been updated!",
    )?;

    if rows_updated > 0 {
        println!("\nNote updated successfully!");
    } else {
        println!("\nNo note found with ID: {}", note_id);
    }

    // READ AGAIN
    let notes = repository.get_notes()?;

    println!("\nNOTES AFTER UPDATE:");

    for note in &notes {
        println!("{:#?}", note);
    }

    // DELETE
    let rows_deleted = repository.delete_note(note_id)?;

    if rows_deleted > 0 {
        println!("\nNote deleted successfully!");
    } else {
        println!("\nNo note found with ID: {}", note_id);
    }

    // FINAL READ
    let notes = repository.get_notes()?;

    println!("\nNOTES AFTER DELETE:");

    for note in &notes {
        println!("{:#?}", note);
    }

    println!("\nOPON database tests completed!");

    Ok(())
}