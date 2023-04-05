use crate::entities::{CheckboxFilterPB, FieldType};
use crate::services::cell::{CellDataChangeset, CellDataDecoder, FromCellString, TypeCellData};
use crate::services::field::{
  default_order, CheckboxCellData, TypeOption, TypeOptionCellData, TypeOptionCellDataCompare,
  TypeOptionCellDataFilter, TypeOptionTransform,
};

use collab::core::lib0_any_ext::Lib0AnyMapExtension;
use collab_database::fields::{Field, TypeOptionData, TypeOptionDataBuilder};
use collab_database::rows::Cell;
use flowy_error::FlowyResult;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CheckboxTypeOption {
  pub is_selected: bool,
}

impl TypeOption for CheckboxTypeOption {
  type CellData = CheckboxCellData;
  type CellChangeset = CheckboxCellChangeset;
  type CellProtobufType = CheckboxCellData;
  type CellFilter = CheckboxFilterPB;
}

impl TypeOptionTransform for CheckboxTypeOption {
  fn transformable(&self) -> bool {
    true
  }

  fn transform_type_option(
    &mut self,
    _old_type_option_field_type: FieldType,
    _old_type_option_data: TypeOptionData,
  ) {
  }

  fn transform_type_option_cell(
    &self,
    cell: &Cell,
    _decoded_field_type: &FieldType,
    _field: &Field,
  ) -> Option<<Self as TypeOption>::CellData> {
    if _decoded_field_type.is_text() {
      Some(CheckboxCellData::from(cell))
    } else {
      None
    }
  }
}

impl From<TypeOptionData> for CheckboxTypeOption {
  fn from(data: TypeOptionData) -> Self {
    let is_selected = data.get_bool_value("is_selected").unwrap_or(false);
    CheckboxTypeOption { is_selected }
  }
}

impl From<CheckboxTypeOption> for TypeOptionData {
  fn from(data: CheckboxTypeOption) -> Self {
    TypeOptionDataBuilder::new()
      .insert("is_selected", data.is_selected)
      .build()
  }
}

impl TypeOptionCellData for CheckboxTypeOption {
  fn convert_to_protobuf(
    &self,
    cell_data: <Self as TypeOption>::CellData,
  ) -> <Self as TypeOption>::CellProtobufType {
    cell_data
  }

  fn decode_cell(&self, cell: &Cell) -> FlowyResult<<Self as TypeOption>::CellData> {
    Ok(CheckboxCellData::from(cell))
  }
}

impl CellDataDecoder for CheckboxTypeOption {
  fn decode_cell_str(
    &self,
    cell: &Cell,
    decoded_field_type: &FieldType,
    field: &Field,
  ) -> FlowyResult<<Self as TypeOption>::CellData> {
    if !decoded_field_type.is_checkbox() {
      return Ok(Default::default());
    }

    self.decode_cell(cell)
  }

  fn decode_cell_data_to_str(&self, cell_data: <Self as TypeOption>::CellData) -> String {
    cell_data.to_string()
  }

  fn decode_cell_to_str(&self, cell: &Cell) -> String {
    Self::CellData::from(cell).to_string()
  }
}

pub type CheckboxCellChangeset = String;

impl CellDataChangeset for CheckboxTypeOption {
  fn apply_changeset(
    &self,
    changeset: <Self as TypeOption>::CellChangeset,
    cell: Option<Cell>,
  ) -> FlowyResult<(Cell, <Self as TypeOption>::CellData)> {
    let checkbox_cell_data = CheckboxCellData::from_str(&changeset)?;
    Ok((checkbox_cell_data.clone().into(), checkbox_cell_data))
  }
}

impl TypeOptionCellDataFilter for CheckboxTypeOption {
  fn apply_filter(
    &self,
    filter: &<Self as TypeOption>::CellFilter,
    field_type: &FieldType,
    cell_data: &<Self as TypeOption>::CellData,
  ) -> bool {
    if !field_type.is_checkbox() {
      return true;
    }
    filter.is_visible(cell_data)
  }
}

impl TypeOptionCellDataCompare for CheckboxTypeOption {
  fn apply_cmp(
    &self,
    cell_data: &<Self as TypeOption>::CellData,
    other_cell_data: &<Self as TypeOption>::CellData,
  ) -> Ordering {
    match (cell_data.is_check(), other_cell_data.is_check()) {
      (true, true) => Ordering::Equal,
      (true, false) => Ordering::Greater,
      (false, true) => Ordering::Less,
      (false, false) => default_order(),
    }
  }
}
