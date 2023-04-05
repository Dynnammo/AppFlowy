use crate::entities::FieldType;
use crate::services::cell::{AtomicCellDataCache, CellProtobufBlob};
use crate::services::field::*;

use crate::services::group::make_no_status_group;
use collab_database::fields::Field;
use collab_database::rows::{get_field_type_from_cell, Cell};

use flowy_error::{ErrorCode, FlowyResult};
use std::fmt::Debug;

/// Decode the opaque cell data into readable format content
pub trait CellDataDecoder: TypeOption {
  ///
  /// Tries to decode the opaque cell string to `decoded_field_type`'s cell data. Sometimes, the `field_type`
  /// of the `FieldRevision` is not equal to the `decoded_field_type`(This happened When switching
  /// the field type of the `FieldRevision` to another field type). So the cell data is need to do
  /// some transformation.
  ///
  /// For example, the current field type of the `FieldRevision` is a checkbox. When switching the field
  /// type from the checkbox to single select, it will create two new options,`Yes` and `No`, if they don't exist.
  /// But the data of the cell doesn't change. We can't iterate all the rows to transform the cell
  /// data that can be parsed by the current field type. One approach is to transform the cell data
  /// when it get read. For the moment, the cell data is a string, `Yes` or `No`. It needs to compare
  /// with the option's name, if match return the id of the option.
  fn decode_cell_str(
    &self,
    cell: &Cell,
    decoded_field_type: &FieldType,
    field: &Field,
  ) -> FlowyResult<<Self as TypeOption>::CellData>;

  /// Same as `decode_cell_data` does but Decode the cell data to readable `String`
  /// For example, The string of the Multi-Select cell will be a list of the option's name
  /// separated by a comma.
  fn decode_cell_data_to_str(&self, cell_data: <Self as TypeOption>::CellData) -> String;

  fn decode_cell_to_str(&self, cell: &Cell) -> String;
}

pub trait CellDataChangeset: TypeOption {
  /// The changeset is able to parse into the concrete data struct if `TypeOption::CellChangeset`
  /// implements the `FromCellChangesetString` trait.
  /// For example,the SelectOptionCellChangeset,DateCellChangeset. etc.
  ///
  fn apply_changeset(
    &self,
    changeset: <Self as TypeOption>::CellChangeset,
    cell: Option<Cell>,
  ) -> FlowyResult<(Cell, <Self as TypeOption>::CellData)>;
}

/// changeset: It will be deserialized into specific data base on the FieldType.
///     For example,
///         FieldType::RichText => String
///         FieldType::SingleSelect => SelectOptionChangeset
///
/// cell_rev: It will be None if the cell does not contain any data.
pub fn apply_cell_data_changeset<C: ToCellChangesetString>(
  changeset: C,
  cell: Option<Cell>,
  field: &Field,
  cell_data_cache: Option<AtomicCellDataCache>,
) -> Cell {
  let changeset = changeset.to_cell_changeset_str();
  let field_type = FieldType::from(field.field_type);
  match TypeOptionCellExt::new_with_cell_data_cache(field, cell_data_cache)
    .get_type_option_cell_data_handler(&field_type)
  {
    None => Cell::default(),
    Some(handler) => handler
      .handle_cell_changeset(changeset, cell, field)
      .unwrap_or_default(),
  }
}

pub fn get_type_cell_protobuf(
  cell: &Cell,
  field: &Field,
  cell_data_cache: Option<AtomicCellDataCache>,
) -> CellProtobufBlob {
  let from_field_type = get_field_type_from_cell(cell);
  if from_field_type.is_none() {
    return CellProtobufBlob::default();
  }

  let from_field_type = from_field_type.unwrap();
  let to_field_type = FieldType::from(field.field_type);
  match try_decode_cell_str_to_cell_protobuf(
    cell,
    &from_field_type,
    &to_field_type,
    field,
    cell_data_cache,
  ) {
    Ok(cell_bytes) => cell_bytes,
    Err(e) => {
      tracing::error!("Decode cell data failed, {:?}", e);
      CellProtobufBlob::default()
    },
  }
}

pub fn get_type_cell_data<Output>(
  cell: &Cell,
  field: &Field,
  cell_data_cache: Option<AtomicCellDataCache>,
) -> Option<Output>
where
  Output: Default + 'static,
{
  let from_field_type = get_field_type_from_cell(&cell)?;
  let to_field_type = FieldType::from(field.field_type);
  try_decode_cell_to_cell_data(
    cell,
    &from_field_type,
    &to_field_type,
    field,
    cell_data_cache,
  )
}

/// Decode the opaque cell data from one field type to another using the corresponding `TypeOption`
///
/// The cell data might become an empty string depends on the to_field_type's `TypeOption`
/// support transform the from_field_type's cell data or not.
///
/// # Arguments
///
/// * `cell_str`: the opaque cell string that can be decoded by corresponding structs that implement the
/// `FromCellString` trait.
/// * `from_field_type`: the original field type of the passed-in cell data. Check the `TypeCellData`
/// that is used to save the origin field type of the cell data.
/// * `to_field_type`: decode the passed-in cell data to this field type. It will use the to_field_type's
/// TypeOption to decode this cell data.
/// * `field_rev`: used to get the corresponding TypeOption for the specified field type.
///
/// returns: CellBytes
///
pub fn try_decode_cell_str_to_cell_protobuf(
  cell: &Cell,
  from_field_type: &FieldType,
  to_field_type: &FieldType,
  field: &Field,
  cell_data_cache: Option<AtomicCellDataCache>,
) -> FlowyResult<CellProtobufBlob> {
  match TypeOptionCellExt::new_with_cell_data_cache(field, cell_data_cache)
    .get_type_option_cell_data_handler(to_field_type)
  {
    None => Ok(CellProtobufBlob::default()),
    Some(handler) => handler.handle_cell_str(cell, from_field_type, field),
  }
}

pub fn try_decode_cell_to_cell_data<T: Default + 'static>(
  cell: &Cell,
  from_field_type: &FieldType,
  to_field_type: &FieldType,
  field: &Field,
  cell_data_cache: Option<AtomicCellDataCache>,
) -> Option<T> {
  let handler = TypeOptionCellExt::new_with_cell_data_cache(field, cell_data_cache)
    .get_type_option_cell_data_handler(to_field_type)?;
  handler
    .get_cell_data(cell, from_field_type, field)
    .ok()?
    .unbox_or_none::<T>()
}
/// Returns a string that represents the current field_type's cell data.
/// For example, The string of the Multi-Select cell will be a list of the option's name
/// separated by a comma.
///
/// # Arguments
///
/// * `cell_str`: the opaque cell string that can be decoded by corresponding structs that implement the
/// `FromCellString` trait.
/// * `decoded_field_type`: the field_type of the cell_str
/// * `field_type`: use this field type's `TypeOption` to stringify this cell_str
/// * `field_rev`: used to get the corresponding TypeOption for the specified field type.
///
/// returns: String
pub fn stringify_cell_data(
  cell: &Cell,
  decoded_field_type: &FieldType,
  field_type: &FieldType,
  field: &Field,
) -> String {
  match TypeOptionCellExt::new_with_cell_data_cache(field, None)
    .get_type_option_cell_data_handler(field_type)
  {
    None => "".to_string(),
    Some(handler) => handler.stringify_cell_str(cell, decoded_field_type, field),
  }
}

pub fn insert_text_cell(s: String, field: &Field) -> Cell {
  apply_cell_data_changeset(s, None, field, None)
}

pub fn insert_number_cell(num: i64, field: &Field) -> Cell {
  apply_cell_data_changeset(num.to_string(), None, field, None)
}

pub fn insert_url_cell(url: String, field: &Field) -> Cell {
  // checking if url is equal to group id of no status group because everywhere
  // except group of rows with empty url the group id is equal to the url
  // so then on the case that url is equal to empty url group id we should change
  // the url to empty string
  let _no_status_group_id = make_no_status_group(field).id;
  let url = match url {
    a if a == _no_status_group_id => "".to_owned(),
    _ => url,
  };

  apply_cell_data_changeset(url, None, field, None)
}

pub fn insert_checkbox_cell(is_check: bool, field: &Field) -> Cell {
  let s = if is_check {
    CHECK.to_string()
  } else {
    UNCHECK.to_string()
  };
  apply_cell_data_changeset(s, None, field, None)
}

pub fn insert_date_cell(timestamp: i64, field: &Field) -> Cell {
  let cell_data = serde_json::to_string(&DateCellChangeset {
    date: Some(timestamp.to_string()),
    time: None,
    include_time: Some(false),
    is_utc: true,
  })
  .unwrap();
  apply_cell_data_changeset(cell_data, None, field, None)
}

pub fn insert_select_option_cell(option_ids: Vec<String>, field: &Field) -> Cell {
  let changeset =
    SelectOptionCellChangeset::from_insert_options(option_ids).to_cell_changeset_str();
  apply_cell_data_changeset(changeset, None, field, None)
}

pub fn delete_select_option_cell(option_ids: Vec<String>, field: &Field) -> Cell {
  let changeset =
    SelectOptionCellChangeset::from_delete_options(option_ids).to_cell_changeset_str();
  apply_cell_data_changeset(changeset, None, field, None)
}

/// Deserialize the String into cell specific data type.
pub trait FromCellString {
  fn from_cell_str(s: &str) -> FlowyResult<Self>
  where
    Self: Sized;
}

/// If the changeset applying to the cell is not String type, it should impl this trait.
/// Deserialize the string into cell specific changeset.
pub trait FromCellChangesetString {
  fn from_changeset(changeset: String) -> FlowyResult<Self>
  where
    Self: Sized;
}

impl FromCellChangesetString for String {
  fn from_changeset(changeset: String) -> FlowyResult<Self>
  where
    Self: Sized,
  {
    Ok(changeset)
  }
}

pub trait ToCellChangesetString: Debug {
  fn to_cell_changeset_str(&self) -> String;
}

impl ToCellChangesetString for String {
  fn to_cell_changeset_str(&self) -> String {
    self.clone()
  }
}

pub struct AnyCellChangeset<T>(pub Option<T>);

impl<T> AnyCellChangeset<T> {
  pub fn try_into_inner(self) -> FlowyResult<T> {
    match self.0 {
      None => Err(ErrorCode::InvalidData.into()),
      Some(data) => Ok(data),
    }
  }
}

impl<T, C: ToString> std::convert::From<C> for AnyCellChangeset<T>
where
  T: FromCellChangesetString,
{
  fn from(changeset: C) -> Self {
    match T::from_changeset(changeset.to_string()) {
      Ok(data) => AnyCellChangeset(Some(data)),
      Err(e) => {
        tracing::error!("Deserialize CellDataChangeset failed: {}", e);
        AnyCellChangeset(None)
      },
    }
  }
}
// impl std::convert::From<String> for AnyCellChangeset<String> {
//     fn from(s: String) -> Self {
//         AnyCellChangeset(Some(s))
//     }
// }