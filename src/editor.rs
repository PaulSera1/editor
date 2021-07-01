/// TODO:
/// INSERT mode [DONE (i think)]
/// find alternative to truncation on overflowing lines (no, it's not OK to assume I use good practice)
/// maybe adding a boolean to control single line right/left scrolling?
/// better status bar (and maybe status bar rendering)
/// - current line number out of total line numbers [DONE]
/// - styling (different color?)
/// - percent of file seems stupid I do not know why ViM does it
/// more useful keybindings
/// syntax highlighting? (make python look intentionally awful)

use std::io::{stdout, Write};
use crossterm::{
  terminal::{
    enable_raw_mode,
    disable_raw_mode,
    size, Clear, ClearType,
    EnterAlternateScreen,
    LeaveAlternateScreen,
  },
  event::{
    KeyCode, Event,
    read, KeyEvent,
    KeyModifiers,
  },
  style::{
    SetForegroundColor, SetBackgroundColor,
    ResetColor,
    Color,
    Print
  },
  cursor::{
    MoveTo, Hide, Show,
    SavePosition, RestorePosition,
    position, SetCursorShape, CursorShape,
    EnableBlinking
  }, execute,
};
use crate::file::{
  Document, Row,
  NLPositionDescriptor, DPositionDescriptor,
  IPositionDescriptor
};
#[allow(unused_imports)]
use super::highlighting;

const NONE: KeyModifiers = KeyModifiers::empty();
const UPPER: KeyModifiers = KeyModifiers::SHIFT;
#[allow(dead_code)]
const VERSION: &str = env!("CARGO_PKG_VERSION");

macro_rules! char_key {
  ($key: pat) => {
    Event::Key(KeyEvent {
      code: KeyCode::Char($key),
      modifiers: NONE
    })
  };
}

macro_rules! char_upper_key {
  ($key: pat) => {
    Event::Key(KeyEvent {
      code: KeyCode::Char($key),
      modifiers: UPPER
    })
  };
}

macro_rules! special_key {
  ($en_t: pat) => {
    Event::Key(KeyEvent {
      code: $en_t,
      modifiers: NONE
    })
  };
}

#[derive(Debug)]
pub struct Terminal {
  height: u16,
  width: u16,
}

impl Terminal {
  pub fn new() -> Result<Self, std::io::Error> {
    let dimensions = size()?;
    enable_raw_mode()?;
    execute!(
      stdout(),
      EnterAlternateScreen
    )?;
    Ok(Self {
      height: dimensions.1,
      width: dimensions.0,
    })
  }

  pub fn size(&self) -> (u16, u16) {
    (self.width, self.height)
  }

  pub fn set_dimensions(&mut self) {
    let (width, height) = size().unwrap();
    self.width = width;
    self.height = height
  }

  pub fn clear(&self) -> Result<(), std::io::Error> {
    Ok(execute!(
      stdout(),
      Clear(ClearType::All)
    )?)
  }
}

#[derive(Debug)]
pub enum EditorMode {
  Normal,
  Command,
  Insert
}

impl EditorMode {
  pub fn to_string(&self) -> String {
    match self {
      EditorMode::Normal => String::from("NORMAL"),
      EditorMode::Command => String::from("COMMAND"),
      EditorMode::Insert => String::from("INSERT")
    }
  }
}

#[derive(Debug)]
pub enum Direction {
  Down,
  Up,
  Left,
  Right
}

#[derive(Debug)]
pub struct StatusBar {
  pub terminal_size: (u16, u16),
  pub cmd: String,
  pub cmd_chars: usize,
  pub mode: EditorMode,
}

impl StatusBar {
  pub fn default() -> Self {
    let dimensions = size().unwrap();
    Self {
      terminal_size: dimensions,
      cmd: String::new(),
      cmd_chars: 0,
      mode: EditorMode::Normal,
    }
  }

  pub fn add_command(&mut self, command: char) {
    self.cmd_chars += 1;
    self.cmd.push(command);
  }

  pub fn remove_command(&mut self) {
    self.cmd_chars -= 1;
    self.cmd.pop();
  }

  pub fn set_mode(&mut self, mode: EditorMode) {
    self.mode = mode
  }

  pub fn render(&self, current: usize, total: usize) {
    let mode_str = self.mode.to_string();
    let line_chars = current.to_string().chars().count() + total.to_string().chars().count();
    let mut stdout = stdout();
    let content = format!("{}{}{}",
      self.cmd,
      (self.cmd_chars..(self.terminal_size.0 as usize - mode_str.len() - line_chars - 4))
        .map(|_| " ")
        .collect::<String>(),
      mode_str);
    print!("{} | ", content);
    let _ = execute!(
      stdout,
      SetBackgroundColor(Color::White),
      SetForegroundColor(Color::Black)
    );
    print!("{}/{}\r", current, total);
    let _ = execute!(
      stdout,
      ResetColor
    );
  }
}

#[derive(Debug)]
pub struct Editor {
  pub terminal: Terminal,
  pub file: Document,
  pub mode: EditorMode,
  pub status_bar: StatusBar,
  altered: bool,
  view_frame: (usize, usize),
  _quit: bool,
  position: (u16, u16),
  buffer: u16
}

impl Editor {
  pub fn new(file_name: Option<&String>) -> Result<Self, std::io::Error> {
    let terminal = Terminal::new()?;
    let terminal_rows = terminal.size().1;
    Ok(Editor {
      terminal,
      altered: false,
      file: if let Some(file_name) = file_name {
        if let Ok(file) = Document::open(file_name) {
          file
        } else {
          Document::new(file_name)
        }
      } else { Document::new("") },
      _quit: false,
      mode: EditorMode::Normal,
      status_bar: StatusBar::default(),
      view_frame: (0, terminal_rows as usize),
      position: (0, 0),
      buffer: 0
    })
  }

  pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    // anything that needs to be done on boot
    execute!(
      stdout(),
      SetCursorShape(CursorShape::Block),
      EnableBlinking
    )?;
    loop {
      if self._quit {
        return Ok(())
      }
      self.terminal.set_dimensions();
      self.render();
      match &self.mode {
        EditorMode::Normal => self.handle_normal(),
        EditorMode::Command => self.handle_command(),
        EditorMode::Insert => self.handle_insert()
      }
      // apparently i'm handling all the main stuff too fast for my terminal
      // the source of the flickering needs to be resolved ASAP
      std::thread::sleep(std::time::Duration::from_millis(1));
    }
  }

  fn write_row(&self, row_no: usize, offset: usize, row: &Row) {
    let mut printed_string = String::from(row.content());
    let mut current_written = 0;
    printed_string.truncate((self.terminal.width - self.buffer - 1) as usize);
    print!("{:indent$}{} ", "", row_no, indent=offset);
    if let Some(highlighted_rows) = self.file.highlighted_rows() {
      for token in highlighted_rows.get(row_no - 1).unwrap() {
        current_written += token.get_original().len(); // temporary
        if current_written > (self.terminal.width - self.buffer - 1) as usize { // temporary
          break
        }
        if let Some(color) = token.get_color() {
          execute!(
            stdout(),
            SetForegroundColor(*color),
            Print(format!("{}", token.get_original())),
            ResetColor
          ).unwrap();
        } else {
          print!("{}", token.get_original())
        }
      }
      print!("\r\n")
    } else {
      print!("{:indent$}{} {}\r\n", "", row_no, printed_string, indent=offset)
    }
  }

  fn write_empty_line(&self) {
    self.clear_row();
    print!("~\r\n")
  }

  fn move_to(&mut self, column: u16, row: u16) {
    execute!(
      stdout(),
      MoveTo(column, row)
    ).unwrap();
    self.position.0 = row;
    self.position.1 = column
  }

  fn handle_command(&mut self) {
    match read().unwrap() {
      char_key!(key) => {
        self.status_bar.add_command(key);
      },
      char_upper_key!(key) => {
        self.status_bar.add_command(key)
      },
      special_key!(KeyCode::Enter) => {
        self.move_to(self.position.1, self.position.0);
        self.evaluate_expr();
        self.mode = EditorMode::Normal;
        self.status_bar.set_mode(EditorMode::Normal);
      },
      special_key!(KeyCode::Esc) => {
        self.mode = EditorMode::Normal;
        self.status_bar.set_mode(EditorMode::Normal)
      },
      special_key!(KeyCode::Backspace) => {
        if self.status_bar.cmd.len() > 0 {
          self.status_bar.remove_command()
        }
      }
      _ => ()
    }
  }

  fn handle_insert(&mut self) {
    match read().unwrap() {
      char_key!(key) | char_upper_key!(key) => self.insert(key),
      special_key!(KeyCode::Backspace) => self.delete(),
      special_key!(KeyCode::Enter) => self.insert_row(),
      special_key!(KeyCode::Esc) => {
        self.mode = EditorMode::Normal;
        let _ = execute!(
          stdout(),
          SetCursorShape(CursorShape::Block),
        );
        self.status_bar.set_mode(EditorMode::Normal)
      },
      special_key!(KeyCode::Down) => {
        self.scroll(Direction::Down)
      },
      special_key!(KeyCode::Up) => {
        self.scroll(Direction::Up)
      },
      special_key!(KeyCode::Left) => {
        self.scroll(Direction::Left)
      },
      special_key!(KeyCode::Right) => {
        self.scroll(Direction::Right)
      },
      _ => () // TODO: all the insert operations, refreshing the buffer
    }
    self.altered = true;
  }

  fn handle_normal(&mut self) {
    match read().unwrap() {
      char_key!('i') => {
        self.mode = EditorMode::Insert;
        let _ = execute!(
          stdout(),
          SetCursorShape(CursorShape::Line)
        );
        self.status_bar.set_mode(EditorMode::Insert)
      },
      char_key!(':') => {
        self.mode = EditorMode::Command;
        self.status_bar.set_mode(EditorMode::Command);
        self.status_bar.add_command(':');
      },
      special_key!(KeyCode::Down) => {
        self.scroll(Direction::Down)
      },
      special_key!(KeyCode::Up) => {
        self.scroll(Direction::Up)
      },
      special_key!(KeyCode::Left) => {
        self.scroll(Direction::Left)
      },
      special_key!(KeyCode::Right) => {
        self.scroll(Direction::Right)
      }
      _ => ()
    }
  }

  fn evaluate_expr(&mut self) {
    if self.status_bar.cmd.starts_with(":set") {
      let copied_cmd = self.status_bar.cmd
      .clone();
    let split_command = copied_cmd.split_whitespace().collect::<Vec<&str>>();
    for idx in 1..split_command.len() {
      match *split_command.get(idx).unwrap() {
        ":set" => (),
        "filename" => {
          if let Some(file_name) = split_command.get(idx + 1) {
            self.file.set_name(*file_name)
          }
          break
        },
        _ => ()
      }
    }
    } else {
      let mut commands = self.status_bar.cmd.chars().rev().collect::<String>();
      while let Some(cmd) = commands.pop() {
        match cmd {
          'q' => if self.file.name() != "" || !self.altered { self._quit = true },
          'g' => self.move_to_beginning(),
          'G' => self.move_to_end(),
          _ => ()
        }
      }
    }
    self.status_bar.cmd.clear();
    self.status_bar.cmd_chars = 0;
  }

  fn scroll(&mut self, direction: Direction) {
    let _ = execute!(
      stdout(),
      Hide
    );
    match direction {
      Direction::Down => {
        let file = &self.file;
        // ensure that we are within the bounds of the file,
        // add one null line to allow buffer to grow
        if self.view_frame.1 - 2 < file.rows.len() as usize
        || self.position.0 + 2 != self.terminal.height {
          // grab row below the current cursor row
          if let Some(row) = file.get_row(self.position.0 as usize + self.view_frame.0 + 1) {
            // 'next_column' ensures stickiness to the left
            let next_column = std::cmp::min(self.position.1, 1 + self.buffer + row.len() as u16);
            // we're at the bottom of the view frame, scroll one line down, move everything else
            if self.position.0 == self.terminal.size().1 - 2 {
              self.view_frame = (self.view_frame.0 + 1, self.view_frame.1 + 1);
              self.position = (self.position.0, next_column);
            } else { // stick left, move cursor left depending on the character length of the next line
              self.position = (self.position.0 + 1, next_column);
            }
            self.move_to(self.position.1, self.position.0)
          }
        }
      },
      Direction::Up => {
        // ensure we are not at the top of the file
        if self.view_frame.0 != 0 || self.position.0 != 0 {
          let file = &self.file;
          // get the row above current cursor
          if let Some(row) = file.get_row(self.position.0 as usize + self.view_frame.0 - 1) {
            // 'next_column' yet again ensures stickiness to the left
            let next_column = std::cmp::min(self.position.1, 1 + self.buffer + row.len() as u16);
            // at the top of the view frame, scroll one line up
            if self.position.0 == 0 {
              self.view_frame = (self.view_frame.0 - 1, self.view_frame.1 - 1);
              self.position = (self.position.0, next_column);
            } else { // stick left, move cursor left depending on the character length of the next line
              self.position = (self.position.0 - 1, next_column);
            }
            self.move_to(self.position.1, self.position.0)
          }
        }
      },
      Direction::Left => {
        if self.position.1 > self.buffer {
          if self.position.1 == self.buffer + 1 {
            if self.position.0 != 0 || self.view_frame.0 != 0 {
              self.scroll(Direction::Up);
              if let Some(row) = self.file
                .get_row(self.view_frame.0 + self.position.0 as usize) {
                  self.position.1 = std::cmp::min(self.terminal.width, self.buffer + 1 + row.len() as u16)
              }
            }
          } else {
            self.position.1 -= 1
          }
          self.move_to(self.position.1, self.position.0)
        }
      },
      Direction::Right => {
        if self.position.1 != self.terminal.width {
          if let Some(row) = self.file
            .get_row(self.view_frame.0 + self.position.0 as usize) {
              if row.len() + self.buffer as usize + 1 == self.position.1 as usize || row.len() == 0 {
                if self.view_frame.0 + self.position.0 as usize == self.file.len() - 1 {
                  return
                }
                self.scroll(Direction::Down);
                self.position.1 = self.buffer + 1;
              } else {
                self.position.1 += 1;
              }
          }
          self.move_to(self.position.1, self.position.0)
        }
      }
    }
  }

  fn move_to_beginning(&mut self) {
    for _ in 0..self.file.len() {
      self.scroll(Direction::Up)
    }
  }

  fn move_to_end(&mut self) {
    for _ in self.view_frame.0..self.file.len() {
      self.scroll(Direction::Down)
    }
  }

  fn clear_row(&self) {
    let _ = execute!(
      stdout(),
      Clear(ClearType::CurrentLine)
    );
  }

  fn get_file_mut(&mut self) -> &mut Document {
    &mut self.file
  }

  fn insert(&mut self, key: char) {
    let line = self.view_frame.0 + self.position.0 as usize;
    let column = self.position.1 - self.buffer - 1;
    let file = self.get_file_mut();
    let row = file.get_row_mut(line).unwrap();
    row.insert(if column as usize == row.len() {
      IPositionDescriptor::End(key)
    } else {
      IPositionDescriptor::Middle(column as usize, key)
    });
    self.scroll(Direction::Right)
  }

  fn delete(&mut self) {
    let line = self.view_frame.0 + self.position.0 as usize;
    let column = self.position.1 - self.buffer - 2;
    let row_length = self.file.rows.get(line).unwrap().len();
    let file = self.get_file_mut();
    if let Some(offset) = file.handle_delete(if column == u16::MAX {
      DPositionDescriptor::Beginning(line)
    } else if column as usize == row_length {
      DPositionDescriptor::End(line)
    } else {
      DPositionDescriptor::Middle(line, column as usize)
    }) {
      self.move_to(offset as u16 + self.buffer + 1, self.position.0 - 1)
    } else {
      self.scroll(Direction::Left)
    }
  }

  fn insert_row(&mut self) {
    let line = self.view_frame.0 + self.position.0 as usize;
    let column = self.position.1 - self.buffer - 2;
    let file = self.get_file_mut();
    let row = file.get_row_mut(line).unwrap();
    let new_row = row.add_new_line(if column as usize == row.len() {
      NLPositionDescriptor::End
    } else if column == u16::MAX {
      NLPositionDescriptor::Beginning
    } else {
      NLPositionDescriptor::Middle(column as usize)
    });
    file.insert_row(line + 1, new_row);
    self.scroll(Direction::Down);
    self.move_to_line_start()
  }

  fn move_to_line_start(&mut self) {
    self.move_to(self.buffer + 1, self.position.0)
  }

  fn render(&mut self) {
    let _ = execute!(
      stdout(),
      Hide,
      SavePosition,
      MoveTo(0, 0),
    );
    let num_rows = self.file.rows.len();
    let buffer = num_rows.to_string().chars().count();
    self.buffer = buffer as u16;
    for terminal_row_no in self.view_frame.0..(self.view_frame.1 - 1) {
      if terminal_row_no < num_rows {
        self.clear_row();
        let used = buffer - (terminal_row_no + 1).to_string().chars().count();
        self.write_row(terminal_row_no + 1, used, self.file.rows.get(terminal_row_no).unwrap());
      } else {
        self.write_empty_line();
      }
    }
    self.status_bar.render(self.view_frame.0 + self.position.0 as usize + 1, self.file.len());
    match self.mode {
      EditorMode::Command => (),
      _ => {
        let _ = execute!(stdout(), RestorePosition, Show);
        let (column, row) = position().unwrap();
        if column == 0 {
          self.position.1 = self.buffer + 1;
          let _ = execute!(
            stdout(),
            MoveTo(self.position.1, row)
          );
        }
      }
    }
    let _ = stdout().flush();
  }
}

impl Drop for Editor {
  fn drop(&mut self) {
    let _ = disable_raw_mode();
    let _ = self.terminal.clear();
    if let Err(why) = self.file.save() {
      eprintln!("{}", why)
    }
    let _ = execute!(
      stdout(),
      SetCursorShape(CursorShape::Block),
      LeaveAlternateScreen,
    );
    // println!("{:?}", self.file.highlighted_rows())
    // println!("{:?}", self.file)
    // println!("row {} column {}", self.position.0, self.position.1);
  }
}
