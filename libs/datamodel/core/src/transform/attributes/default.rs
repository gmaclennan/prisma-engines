use super::{super::helpers::*, AttributeValidator};
use crate::{
    ast::{self, Span},
    diagnostics::DatamodelError,
    dml,
};
use prisma_value::PrismaValue;

/// Prismas builtin `@default` attribute.
pub struct DefaultAttributeValidator;

impl AttributeValidator<dml::Field> for DefaultAttributeValidator {
    fn attribute_name(&self) -> &'static str {
        &"default"
    }

    fn validate_and_apply(&self, args: &mut Arguments<'_>, field: &mut dml::Field) -> Result<(), DatamodelError> {
        if let dml::Field::RelationField(_) = field {
            return self.new_attribute_validation_error("Cannot set a default value on a relation field.", args.span());
        } else if let dml::Field::ScalarField(sf) = field {
            // If we allow list default values, we need to adjust the types below properly for that case.
            if sf.arity == dml::FieldArity::List {
                return self.new_attribute_validation_error("Cannot set a default value on list field.", args.span());
            }

            if let dml::FieldType::Base(scalar_type, _) = sf.field_type {
                let dv = args
                    .default_arg("value")?
                    .as_default_value_for_scalar_type(scalar_type)
                    .map_err(|e| self.wrap_in_attribute_validation_error(&e))?;

                sf.default_value = Some(dv);
            } else if let dml::FieldType::NativeType(scalar_type, _) = sf.field_type {
                let dv = args
                    .default_arg("value")?
                    .as_default_value_for_scalar_type(scalar_type)
                    .map_err(|e| self.wrap_in_attribute_validation_error(&e))?;

                sf.default_value = Some(dv);
            } else if let dml::FieldType::Enum(_) = sf.field_type {
                let default_arg = args.default_arg("value")?;

                match default_arg.as_constant_literal() {
                    Ok(value) => sf.default_value = Some(dml::DefaultValue::Single(PrismaValue::Enum(value))),
                    Err(err) => {
                        let generator = default_arg.as_value_generator()?;
                        if generator.is_dbgenerated() {
                            sf.default_value = Some(dml::DefaultValue::Expression(generator));
                        } else {
                            return Err(self.wrap_in_attribute_validation_error(&err));
                        }
                    }
                }
            } else if let dml::FieldType::Unsupported(_) = sf.field_type {
                let default_arg = args.default_arg("value")?;

                match default_arg.as_value_generator() {
                    Ok(generator) if generator.is_dbgenerated() => {
                        sf.default_value = Some(dml::DefaultValue::Expression(generator))
                    }
                    Err(e) => return Err(self.wrap_in_attribute_validation_error(&e)),
                    _ => {
                        return self
                            .new_attribute_validation_error("Invalid default value on Unsupported type.", args.span())
                    }
                }
            }
        }

        validate_dbgenerated(self, field, args.span())?;

        Ok(())
    }

    fn serialize(&self, field: &dml::Field, _datamodel: &dml::Datamodel) -> Vec<ast::Attribute> {
        if let Some(default_value) = field.default_value() {
            return vec![ast::Attribute::new(
                self.attribute_name(),
                vec![ast::Argument::new("", lower_default_value(default_value.clone()))],
            )];
        }

        vec![]
    }
}

fn validate_dbgenerated(
    validator: &DefaultAttributeValidator,
    field: &dml::Field,
    span: Span,
) -> Result<(), DatamodelError> {
    if let Some("") = field
        .default_value()
        .and_then(|v| v.as_expression())
        .and_then(|e| e.as_dbgenerated())
    {
        return validator.new_attribute_validation_error(
            "dbgenerated() takes either no argument, or a single nonempty string argument.",
            span,
        );
    }

    Ok(())
}

pub fn lower_default_value(dv: dml::DefaultValue) -> ast::Expression {
    match dv {
        dml::DefaultValue::Single(v) => lower_prisma_value(&v),
        dml::DefaultValue::Expression(e) => {
            let exprs = e.args.iter().map(lower_prisma_value).collect();
            ast::Expression::Function(e.name, exprs, ast::Span::empty())
        }
    }
}

pub fn lower_prisma_value(pv: &PrismaValue) -> ast::Expression {
    match pv {
        PrismaValue::Boolean(true) => ast::Expression::BooleanValue(String::from("true"), ast::Span::empty()),
        PrismaValue::Boolean(false) => ast::Expression::BooleanValue(String::from("false"), ast::Span::empty()),
        PrismaValue::String(value) => ast::Expression::StringValue(value.clone(), ast::Span::empty()),
        PrismaValue::Enum(value) => ast::Expression::ConstantValue(value.clone(), ast::Span::empty()),
        PrismaValue::DateTime(value) => ast::Expression::StringValue(value.to_rfc3339(), ast::Span::empty()),
        PrismaValue::Float(value) => ast::Expression::NumericValue(value.to_string(), ast::Span::empty()),
        PrismaValue::Int(value) => ast::Expression::NumericValue(value.to_string(), ast::Span::empty()),
        PrismaValue::BigInt(value) => ast::Expression::NumericValue(value.to_string(), ast::Span::empty()),
        PrismaValue::Null => ast::Expression::ConstantValue("null".to_string(), ast::Span::empty()),
        PrismaValue::Uuid(val) => ast::Expression::StringValue(val.to_string(), ast::Span::empty()),
        PrismaValue::Json(val) => ast::Expression::StringValue(val.to_string(), ast::Span::empty()),
        PrismaValue::List(vec) => ast::Expression::Array(
            vec.iter().map(|pv| lower_prisma_value(pv)).collect(),
            ast::Span::empty(),
        ),
        PrismaValue::Xml(val) => ast::Expression::StringValue(val.to_string(), ast::Span::empty()),
        PrismaValue::Bytes(b) => ast::Expression::StringValue(prisma_value::encode_bytes(b), ast::Span::empty()),
    }
}
