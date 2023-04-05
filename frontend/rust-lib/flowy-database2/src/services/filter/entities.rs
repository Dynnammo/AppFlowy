use crate::entities::{
  AlterFilterParams, DatabaseSettingChangesetParams, DeleteFilterParams, FieldType, InsertedRowPB,
};
use collab_database::fields::Field;

#[derive(Debug)]
pub struct FilterChangeset {
  pub(crate) insert_filter: Option<FilterType>,
  pub(crate) update_filter: Option<UpdatedFilterType>,
  pub(crate) delete_filter: Option<FilterType>,
}

#[derive(Debug)]
pub struct UpdatedFilterType {
  pub old: Option<FilterType>,
  pub new: FilterType,
}

impl UpdatedFilterType {
  pub fn new(old: Option<FilterType>, new: FilterType) -> UpdatedFilterType {
    Self { old, new }
  }
}

impl FilterChangeset {
  pub fn from_insert(filter_type: FilterType) -> Self {
    Self {
      insert_filter: Some(filter_type),
      update_filter: None,
      delete_filter: None,
    }
  }

  pub fn from_update(filter_type: UpdatedFilterType) -> Self {
    Self {
      insert_filter: None,
      update_filter: Some(filter_type),
      delete_filter: None,
    }
  }
  pub fn from_delete(filter_type: FilterType) -> Self {
    Self {
      insert_filter: None,
      update_filter: None,
      delete_filter: Some(filter_type),
    }
  }
}

impl std::convert::From<&DatabaseSettingChangesetParams> for FilterChangeset {
  fn from(params: &DatabaseSettingChangesetParams) -> Self {
    let insert_filter = params
      .insert_filter
      .as_ref()
      .map(|insert_filter_params| FilterType {
        field_id: insert_filter_params.field_id.clone(),
        field_type: insert_filter_params.field_type.clone(),
      });

    let delete_filter = params
      .delete_filter
      .as_ref()
      .map(|delete_filter_params| delete_filter_params.filter_type.clone());
    FilterChangeset {
      insert_filter,
      update_filter: None,
      delete_filter,
    }
  }
}

#[derive(Hash, Eq, PartialEq, Debug, Clone)]
pub struct FilterType {
  pub field_id: String,
  pub field_type: FieldType,
}

impl std::convert::From<&Field> for FilterType {
  fn from(field: &Field) -> Self {
    Self {
      field_id: field.id.clone(),
      field_type: FieldType::from(field.field_type),
    }
  }
}

impl std::convert::From<&AlterFilterParams> for FilterType {
  fn from(params: &AlterFilterParams) -> Self {
    let field_type: FieldType = params.field_type.clone();
    Self {
      field_id: params.field_id.clone(),
      field_type,
    }
  }
}

impl std::convert::From<&DeleteFilterParams> for FilterType {
  fn from(params: &DeleteFilterParams) -> Self {
    params.filter_type.clone()
  }
}

#[derive(Clone, Debug)]
pub struct FilterResultNotification {
  pub view_id: String,

  // Indicates there will be some new rows being visible from invisible state.
  pub visible_rows: Vec<InsertedRowPB>,

  // Indicates there will be some new rows being invisible from visible state.
  pub invisible_rows: Vec<String>,
}

impl FilterResultNotification {
  pub fn new(view_id: String) -> Self {
    Self {
      view_id,
      visible_rows: vec![],
      invisible_rows: vec![],
    }
  }
}