#include "MassesModel.hpp"
#include "solver/BowModel.hpp"

MassesModel::MassesModel(Masses& masses) {
    ARROW            = addCustom(masses.arrow);
    LIMB_TIP_UPPER   = addDouble(masses.limb_tip_upper);
    LIMB_TIP_LOWER   = addDouble(masses.limb_tip_lower);
    STRING_NOCK      = addDouble(masses.string_nock);
    STRING_TIP_UPPER = addDouble(masses.string_tip_upper);
    STRING_TIP_LOWER = addDouble(masses.string_tip_lower);
}
