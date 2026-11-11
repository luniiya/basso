//! Contains a few ways to style numbers. At the time of writing, Minecraft only
//! uses this for rendering scoreboard objectives.

#[cfg(feature = "basso-buf")]
use std::io::{self, Cursor, Write};

#[cfg(feature = "basso-buf")]
use basso_buf::AzBuf;
#[cfg(feature = "basso-buf")]
use basso_registry::builtin::NumberFormatKind;
use simdnbt::owned::Nbt;

use crate::FormattedText;

#[derive(Clone, Debug, PartialEq)]
pub enum NumberFormat {
    Blank,
    Styled { style: Nbt },
    Fixed { value: FormattedText },
}

#[cfg(feature = "basso-buf")]
impl AzBuf for NumberFormat {
    fn azalea_read(buf: &mut Cursor<&[u8]>) -> Result<Self, basso_buf::BufReadError> {
        let kind = NumberFormatKind::azalea_read(buf)?;
        match kind {
            NumberFormatKind::Blank => Ok(NumberFormat::Blank),
            NumberFormatKind::Styled => Ok(NumberFormat::Styled {
                style: simdnbt::owned::read(buf)?,
            }),
            NumberFormatKind::Fixed => Ok(NumberFormat::Fixed {
                value: FormattedText::azalea_read(buf)?,
            }),
        }
    }
    fn azalea_write(&self, buf: &mut impl Write) -> io::Result<()> {
        match self {
            NumberFormat::Blank => NumberFormatKind::Blank.azalea_write(buf)?,
            NumberFormat::Styled { style } => {
                NumberFormatKind::Styled.azalea_write(buf)?;
                style.azalea_write(buf)?;
            }
            NumberFormat::Fixed { value } => {
                NumberFormatKind::Fixed.azalea_write(buf)?;
                value.azalea_write(buf)?;
            }
        }
        Ok(())
    }
}
